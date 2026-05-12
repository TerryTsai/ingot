//! Boot image — the in-memory bringup that a Linux x86_64 guest starts in.
//! `Image::install` is the canonical view: scrolling top-to-bottom walks
//! guest memory from low to high address, watching bytes appear at each
//! region. Payloads (kernel ELF, initramfs) load in a small prelude; the
//! rest of `install` is pure byte assembly through `Asm`.
//!
//! Sparse layouts (boot_params, page tables) only write the fields they
//! care about and rely on Memory being zero-initialized by `MAP_ANONYMOUS`.
//! Do not run an `Image` against pre-written memory without clearing it.

use std::path::Path;

use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::kvm::RegState;
use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

use super::asm::Asm;
use super::layout::{
    e820_entries, BOOT_PARAMS_ADDR, CMDLINE_ADDR, CMDLINE_MAX, EBDA_START,
    GDT_ADDR, IOAPIC_ADDR, LAPIC_ADDR, PDPT_ADDR, PML4_ADDR,
};

// --- ELF ---
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

// --- vCPU init ---
const CR0_PE: u64 = 1 << 0;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
/// xAPIC at default address, globally enabled, BSP. Leaving APIC_BASE at 0
/// disables the in-kernel LAPIC and forces every LAPIC MMIO out to userspace.
const APIC_BASE_BSP_ENABLED: u64 = 0xfee0_0000 | 0x800 | 0x100;
const GDT_LIMIT: u16 = 23;
const CODE_SELECTOR: u16 = 0x10;
const DATA_SELECTOR: u16 = 0x18;

pub(crate) struct Image<'m> {
    mem: &'m mut Memory,
    kernel_path: &'m Path,
    initramfs_path: Option<&'m Path>,
    cmdline: &'m str,
    vcpus: u8,
    mem_size: u64,
}

impl<'m> Image<'m> {
    pub fn new(
        mem: &'m mut Memory,
        kernel_path: &'m Path,
        initramfs_path: Option<&'m Path>,
        cmdline: &'m str,
        vcpus: u8,
        mem_size: u64,
    ) -> Self {
        Self { mem, kernel_path, initramfs_path, cmdline, vcpus, mem_size }
    }

    pub fn install(self) -> Result<RegState, HostError> {
        let Image { mem, kernel_path, initramfs_path, cmdline, vcpus, mem_size } = self;

        // Phase 1 — file-loaded payloads. Their addresses come back as data.
        let (kernel_entry, kernel_end) = load_kernel(mem, kernel_path)?;
        let initramfs = load_initramfs(mem, initramfs_path, kernel_end)?;
        let e820 = e820_entries(mem_size)?;

        // Phase 2 — assemble the bringup region in physical address order.
        let mut a = Asm::new(mem);

        // ─── 0x0000_0500  GDT (24 B) ────────────────────────────────────
        a.org(GDT_ADDR);
        a.dq(0);                                            // null
        a.dq(0x00AF_9A00_0000_FFFF);                        // code: L + P + exec
        a.dq(0x00CF_9200_0000_FFFF);                        // data: P + writable

        // ─── 0x0000_7000  boot_params (sparse) ──────────────────────────
        a.org(BOOT_PARAMS_ADDR);
        a.b(0x210, 0xFF);                                   // type_of_loader
        a.w(0x1FE, 0xAA55);                                 // boot_flag
        a.d(0x202, 0x53726448);                             // "HdrS"
        a.w(0x206, 0x020F);                                 // version 2.15
        a.d(0x228, CMDLINE_ADDR as u32);                    // cmd_line_ptr (lo)
        a.d(0x0C8, (CMDLINE_ADDR >> 32) as u32);            // cmd_line_ptr (hi)
        a.d(0x238, CMDLINE_MAX as u32);                     // cmdline_size
        if let Some((start, size)) = initramfs {
            a.d(0x218, start as u32);                       // ramdisk_image (lo)
            a.d(0x0C0, (start >> 32) as u32);               // ramdisk_image (hi)
            a.d(0x21C, size as u32);                        // ramdisk_size
        }
        a.b(0x1E8, e820.len() as u8);                       // e820_entries
        for (i, e) in e820.iter().enumerate() {
            let off = 0x2D0 + i as u64 * 20;
            a.q(off,      e.addr);
            a.q(off + 8,  e.size);
            a.d(off + 16, e.kind);
        }

        // ─── 0x0000_9000  PML4 + PDPT (sparse, identity-maps low 4 GiB) ─
        a.org(PML4_ADDR);
        a.q(0, PDPT_ADDR | 0x3);                            // PML4[0] → PDPT (P+RW)
        for i in 0..4 {
            a.q(0x1000 + i as u64 * 8, ((i as u64) * 0x4000_0000) | 0x83);
        }

        // ─── 0x0002_0000  cmdline (dense) ───────────────────────────────
        a.org(CMDLINE_ADDR);
        a.raw(cmdline.as_bytes());
        a.db(0);

        // ─── 0x0009_FC00  MP table (dense, checksums back-patched) ──────
        a.org(EBDA_START);
        let fp = a.here();
        a.raw(b"_MP_");
        a.dd(EBDA_START as u32 + 16);
        a.raw(&[1, 4, 0, 0, 0, 0, 0, 0]);
        a.b(10, a.cksum(fp..a.here()));
        let cfg = a.here();
        a.raw(b"PCMP");
        a.raw(&[0; 4]);                                     // cfg_len, spec_rev, checksum
        a.raw(b"INGOT   ");
        a.raw(b"VMM         ");
        a.raw(&[0; 8]);                                     // oem_table_ptr, oem_size, entry_count
        a.dd(LAPIC_ADDR);
        a.raw(&[0; 4]);                                     // ext_len, ext_checksum
        for i in 0..vcpus {
            a.raw(&[0, i, 0x14, if i == 0 { 0x03 } else { 0x01 }]);
            a.dd(0x600);
            a.dd(0x0fab_fbff);
            a.raw(&[0u8; 8]);
        }
        a.raw(&[1, 0]); a.raw(b"ISA   ");                   // bus
        a.raw(&[2, 0, 0x11, 0x01]); a.dd(IOAPIC_ADDR);      // ioapic
        let cfg_end = a.here();
        a.org(cfg);
        a.w(4,  (cfg_end - cfg) as u16);
        a.w(34, vcpus as u16 + 2);
        a.b(7,  a.cksum(cfg..cfg_end));

        Ok(reg_state_for(kernel_entry))
    }
}

// ── file-loaded payloads ─────────────────────────────────────────────────

/// Read the ELF at `path` and copy each PT_LOAD segment to its p_paddr.
/// Returns `(entry, end)`.
fn load_kernel(mem: &mut Memory, path: &Path) -> Result<(u64, u64), HostError> {
    let bytes = std::fs::read(path)
        .map_err(|_| HostError::KernelImageNotReadable(path.to_path_buf()))?;
    validate_elf_header(&bytes)?;
    let (entry, e_phoff, e_phnum) = elf_header_fields(&bytes);
    let mut end = 0u64;
    for i in 0..e_phnum {
        let phdr = parse_phdr(&bytes, e_phoff + i * PHDR_SIZE)?;
        if phdr.p_type != PT_LOAD { continue; }
        if phdr.p_offset + phdr.p_filesz > bytes.len() {
            return Err(HostError::InvalidConfig("ELF: PT_LOAD past file end"));
        }
        mem.write(phdr.p_paddr, &bytes[phdr.p_offset..phdr.p_offset + phdr.p_filesz])
            .map_err(|_| HostError::InvalidConfig("kernel segment write failed"))?;
        end = end.max(phdr.p_paddr + phdr.p_memsz);
    }
    Ok((entry, end))
}

/// Place the initramfs page-aligned after `kernel_end`. Returns `(start, size)`,
/// or `None` if no initramfs path was configured.
fn load_initramfs(
    mem: &mut Memory,
    path: Option<&Path>,
    kernel_end: u64,
) -> Result<Option<(u64, u64)>, HostError> {
    let Some(path) = path else { return Ok(None); };
    let bytes = std::fs::read(path)
        .map_err(|_| HostError::InitramfsNotReadable(path.to_path_buf()))?;
    let start = (kernel_end + 0xFFF) & !0xFFF;
    let size = bytes.len() as u64;
    if start.checked_add(size).is_none_or(|e| e > mem.size()) {
        return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
    }
    mem.write(start, &bytes)
        .map_err(|_| HostError::InvalidConfig("initramfs write failed"))?;
    Ok(Some((start, size)))
}

// ── ELF parsing helpers ──────────────────────────────────────────────────

struct Phdr { p_type: u32, p_offset: usize, p_paddr: u64, p_filesz: usize, p_memsz: u64 }

fn validate_elf_header(image: &[u8]) -> Result<(), HostError> {
    if image.len() < EHDR_SIZE      { return Err(HostError::InvalidConfig("ELF: truncated header")); }
    if image[0..4] != ELF_MAGIC     { return Err(HostError::InvalidConfig("ELF: bad magic")); }
    if image[4] != ELFCLASS64       { return Err(HostError::InvalidConfig("ELF: not 64-bit")); }
    if image[5] != ELFDATA2LSB      { return Err(HostError::InvalidConfig("ELF: not little-endian")); }
    let e_type = u16::from_le_bytes([image[16], image[17]]);
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err(HostError::InvalidConfig("ELF: not executable"));
    }
    let e_machine = u16::from_le_bytes([image[18], image[19]]);
    if e_machine != EM_X86_64 { return Err(HostError::InvalidConfig("ELF: not x86_64")); }
    Ok(())
}

fn elf_header_fields(image: &[u8]) -> (u64, usize, usize) {
    let e_entry = u64::from_le_bytes(image[24..32].try_into().unwrap());
    let e_phoff = u64::from_le_bytes(image[32..40].try_into().unwrap()) as usize;
    let e_phnum = u16::from_le_bytes([image[56], image[57]]) as usize;
    (e_entry, e_phoff, e_phnum)
}

fn parse_phdr(image: &[u8], off: usize) -> Result<Phdr, HostError> {
    if off + PHDR_SIZE > image.len() {
        return Err(HostError::InvalidConfig("ELF: program header out of file"));
    }
    Ok(Phdr {
        p_type:   u32::from_le_bytes(image[off..off + 4].try_into().unwrap()),
        p_offset: u64::from_le_bytes(image[off + 8..off + 16].try_into().unwrap()) as usize,
        p_paddr:  u64::from_le_bytes(image[off + 24..off + 32].try_into().unwrap()),
        p_filesz: u64::from_le_bytes(image[off + 32..off + 40].try_into().unwrap()) as usize,
        p_memsz:  u64::from_le_bytes(image[off + 40..off + 48].try_into().unwrap()),
    })
}

// ── vCPU register state ──────────────────────────────────────────────────

fn reg_state_for(kernel_entry: u64) -> RegState {
    let mut regs: kvm_regs = unsafe { std::mem::zeroed() };
    regs.rip = kernel_entry;
    regs.rsi = BOOT_PARAMS_ADDR;
    regs.rflags = 0x2;

    let mut sregs: kvm_sregs = unsafe { std::mem::zeroed() };
    sregs.cs = code_segment(CODE_SELECTOR);
    sregs.ds = data_segment(DATA_SELECTOR);
    sregs.es = data_segment(DATA_SELECTOR);
    sregs.fs = data_segment(DATA_SELECTOR);
    sregs.gs = data_segment(DATA_SELECTOR);
    sregs.ss = data_segment(DATA_SELECTOR);
    sregs.tr.limit = 0xFFFF;
    sregs.tr.type_ = 0xB;
    sregs.tr.present = 1;
    sregs.ldt.unusable = 1;
    sregs.gdt.base = GDT_ADDR;
    sregs.gdt.limit = GDT_LIMIT;
    sregs.cr0 = CR0_PE | CR0_NE | CR0_PG;
    sregs.cr3 = PML4_ADDR;
    sregs.cr4 = CR4_PAE;
    sregs.efer = EFER_LME | EFER_LMA;
    sregs.apic_base = APIC_BASE_BSP_ENABLED;

    RegState { regs, sregs }
}

fn code_segment(selector: u16) -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF; s.selector = selector; s.type_ = 0xA;
    s.present = 1; s.s = 1; s.l = 1; s.g = 1;
    s
}

fn data_segment(selector: u16) -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF; s.selector = selector; s.type_ = 0x2;
    s.present = 1; s.s = 1; s.db = 1; s.g = 1;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::layout::KERNEL_BASE;

    const MEM_SIZE: u64 = 128 * 1024 * 1024;

    fn synth_elf(p_paddr: u64, payload: &[u8], p_memsz: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + 56 + payload.len());
        b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u16.to_le_bytes());            // e_type
        b.extend_from_slice(&62u16.to_le_bytes());           // e_machine
        b.extend_from_slice(&1u32.to_le_bytes());            // e_version
        b.extend_from_slice(&p_paddr.to_le_bytes());         // e_entry
        b.extend_from_slice(&64u64.to_le_bytes());           // e_phoff
        b.extend_from_slice(&0u64.to_le_bytes());            // e_shoff
        b.extend_from_slice(&0u32.to_le_bytes());            // e_flags
        b.extend_from_slice(&64u16.to_le_bytes());           // e_ehsize
        b.extend_from_slice(&56u16.to_le_bytes());           // e_phentsize
        b.extend_from_slice(&1u16.to_le_bytes());            // e_phnum
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&1u32.to_le_bytes());            // p_type=PT_LOAD
        b.extend_from_slice(&5u32.to_le_bytes());            // p_flags
        b.extend_from_slice(&(64u64 + 56).to_le_bytes());    // p_offset
        b.extend_from_slice(&p_paddr.to_le_bytes());         // p_vaddr
        b.extend_from_slice(&p_paddr.to_le_bytes());         // p_paddr
        b.extend_from_slice(&(payload.len() as u64).to_le_bytes()); // p_filesz
        b.extend_from_slice(&p_memsz.to_le_bytes());
        b.extend_from_slice(&0x1000u64.to_le_bytes());       // p_align
        b.extend_from_slice(payload);
        b
    }

    fn write_temp_elf(name: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-image-{}-{}", name, std::process::id()));
        std::fs::write(&p, synth_elf(KERNEL_BASE, &[0xf4], 16)).unwrap();
        p
    }

    #[test]
    fn install_writes_gdt_descriptors() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("gdt");
        Image::new(&mut mem, &path, None, "", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        let mut buf = [0u8; 24];
        mem.read(GDT_ADDR, &mut buf).unwrap();
        assert_eq!(u64::from_le_bytes(buf[0..8].try_into().unwrap()), 0);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x00AF_9A00_0000_FFFF);
        assert_eq!(u64::from_le_bytes(buf[16..24].try_into().unwrap()), 0x00CF_9200_0000_FFFF);
    }

    #[test]
    fn install_writes_boot_params_signature_and_cmdline_pointer() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("bp");
        Image::new(&mut mem, &path, None, "ttyS0", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        let mut buf = [0u8; 4096];
        mem.read(BOOT_PARAMS_ADDR, &mut buf).unwrap();
        assert_eq!(buf[0x210], 0xFF);
        assert_eq!(u16::from_le_bytes([buf[0x1FE], buf[0x1FF]]), 0xAA55);
        assert_eq!(u32::from_le_bytes(buf[0x202..0x206].try_into().unwrap()), 0x53726448);
        assert_eq!(u32::from_le_bytes(buf[0x228..0x22C].try_into().unwrap()), CMDLINE_ADDR as u32);
        assert_eq!(buf[0x1E8], 3);
    }

    #[test]
    fn install_writes_page_tables_identity_mapping() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("pt");
        Image::new(&mut mem, &path, None, "", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        let mut pml4 = [0u8; 8];
        mem.read(PML4_ADDR, &mut pml4).unwrap();
        assert_eq!(u64::from_le_bytes(pml4) & !0xFFF, PDPT_ADDR);
        for i in 0..4 {
            let mut e = [0u8; 8];
            mem.read(PDPT_ADDR + i as u64 * 8, &mut e).unwrap();
            let v = u64::from_le_bytes(e);
            assert_eq!(v & !0x3FFF_FFFF, (i as u64) * 0x4000_0000);
            assert_eq!(v & 0x83, 0x83);
        }
    }

    #[test]
    fn install_writes_cmdline_string_with_nul() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("cmd");
        Image::new(&mut mem, &path, None, "console=ttyS0", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        let mut buf = [0u8; 14];
        mem.read(CMDLINE_ADDR, &mut buf).unwrap();
        assert_eq!(&buf[..13], b"console=ttyS0");
        assert_eq!(buf[13], 0);
    }

    #[test]
    fn install_writes_mp_table_signatures_and_valid_checksums() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("mp");
        Image::new(&mut mem, &path, None, "", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        // floating pointer (16 B; checksum byte at +10 makes sum ≡ 0 mod 256)
        let mut fp = [0u8; 16];
        mem.read(EBDA_START, &mut fp).unwrap();
        assert_eq!(&fp[..4], b"_MP_");
        assert_eq!(fp.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        // config header — read length first, then full body, verify checksum
        let mut hdr = [0u8; 6];
        mem.read(EBDA_START + 16, &mut hdr).unwrap();
        assert_eq!(&hdr[..4], b"PCMP");
        let cfg_len = u16::from_le_bytes(hdr[4..6].try_into().unwrap()) as usize;
        let mut cfg = vec![0u8; cfg_len];
        mem.read(EBDA_START + 16, &mut cfg).unwrap();
        assert_eq!(cfg.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        assert_eq!(u16::from_le_bytes([cfg[34], cfg[35]]), 3);
    }

    #[test]
    fn install_returns_reg_state_with_kernel_entry_and_long_mode() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("regs");
        let state = Image::new(&mut mem, &path, None, "", 1, MEM_SIZE).install().unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(state.regs.rip, KERNEL_BASE);
        assert_eq!(state.regs.rsi, BOOT_PARAMS_ADDR);
        assert_ne!(state.sregs.cr0 & CR0_PG, 0);
        assert_eq!(state.sregs.cr3, PML4_ADDR);
        assert_ne!(state.sregs.efer & EFER_LME, 0);
        assert_eq!(state.sregs.cs.l, 1);
    }
}
