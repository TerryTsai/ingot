//! Boot image — phase-2 byte assembly of the bringup region. `Image`
//! is the orchestrator: phase 1 calls the file loaders in `loaders/`
//! (kernel ELF, initramfs); phase 2 lays the bringup tables (GDT,
//! boot_params, page tables, cmdline, MP table) into guest memory
//! through `Asm`; the final step calls `registers::Registers::for_kernel`
//! for the vCPU state.
//!
//! `install()` reads top-to-bottom as a memory map of the bringup
//! region — scrolling walks guest memory from low to high address.
//! Sparse layouts (boot_params, page tables) only write the fields
//! they care about and rely on Memory being zero-initialized by
//! `MAP_ANONYMOUS`. Do not run an `Image` against pre-written memory
//! without clearing it.

use std::path::Path;

use crate::kvm::RegState;
use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

use super::asm::Asm;
use super::layout::{
    e820_entries, BOOT_PARAMS_ADDR, CMDLINE_ADDR, CMDLINE_MAX, EBDA_START,
    GDT_ADDR, IOAPIC_ADDR, LAPIC_ADDR, PDPT_ADDR, PML4_ADDR,
};
use super::loaders::{Initramfs, Kernel};
use super::registers::Registers;

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

        // Phase 1 — file-loaded payloads. Loaders place bytes and
        // return placement records the bringup tables reference.
        let kernel = Kernel::load(mem, kernel_path)?;
        let initramfs = match initramfs_path {
            Some(p) => Some(Initramfs::load(mem, p, kernel.end)?),
            None => None,
        };
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
        if let Some(img) = &initramfs {
            a.d(0x218, img.start as u32);                   // ramdisk_image (lo)
            a.d(0x0C0, (img.start >> 32) as u32);           // ramdisk_image (hi)
            a.d(0x21C, img.size as u32);                    // ramdisk_size
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

        Ok(Registers::for_kernel(kernel.entry))
    }
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
        let mut fp = [0u8; 16];
        mem.read(EBDA_START, &mut fp).unwrap();
        assert_eq!(&fp[..4], b"_MP_");
        assert_eq!(fp.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        let mut hdr = [0u8; 6];
        mem.read(EBDA_START + 16, &mut hdr).unwrap();
        assert_eq!(&hdr[..4], b"PCMP");
        let cfg_len = u16::from_le_bytes(hdr[4..6].try_into().unwrap()) as usize;
        let mut cfg = vec![0u8; cfg_len];
        mem.read(EBDA_START + 16, &mut cfg).unwrap();
        assert_eq!(cfg.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        assert_eq!(u16::from_le_bytes([cfg[34], cfg[35]]), 3);
    }
}
