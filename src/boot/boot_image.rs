//! In-memory bringup image for a Linux x86_64 guest. Owns a mutable
//! borrow of guest memory plus the config inputs needed to materialize
//! every fixed-address structure the kernel reads at entry (page
//! tables, GDT, cmdline, boot_params, MP table) and to place the ELF
//! kernel + optional initramfs. `install` is the only public entry;
//! every byte layout lives in a private `build_*` method, called
//! against a `write` helper that resolves to `self.mem.write` at the
//! address taken from `layout::`.

use std::path::{Path, PathBuf};

use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::kvm::RegState;
use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

use super::layout::{
    e820_entries, BOOT_PARAMS_ADDR, CMDLINE_ADDR, CMDLINE_MAX, EBDA_START,
    GDT_ADDR, IOAPIC_ADDR, LAPIC_ADDR, PDPT_ADDR, PML4_ADDR,
};

// --- ELF -----------------------------------------------------------------
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

// --- GDT -----------------------------------------------------------------
const NULL_DESCRIPTOR: u64 = 0;
const CODE_64BIT: u64 = 0x00AF_9A00_0000_FFFF;
const DATA_64BIT: u64 = 0x00CF_9200_0000_FFFF;
pub(super) const GDT_LIMIT: u16 = 23;
pub(super) const CODE_SELECTOR: u16 = 0x10;
pub(super) const DATA_SELECTOR: u16 = 0x18;

// --- Page tables ---------------------------------------------------------
const PT_PRESENT: u64 = 1 << 0;
const PT_RW: u64 = 1 << 1;
const PT_PAGE_SIZE: u64 = 1 << 7;
const PAGE_1GIB: u64 = 0x4000_0000;
const PT_BUFFER_SIZE: usize = 0x2000; // PML4 (0x1000) + PDPT (0x1000), contiguous

// --- Boot params ---------------------------------------------------------
const BP_SIZE: usize = 4096;
const BP_E820_ENTRIES_OFF: usize = 0x1E8;
const BP_EXT_RAMDISK_IMAGE: usize = 0x0C0;
const BP_EXT_RAMDISK_SIZE: usize = 0x0C4;
const BP_EXT_CMD_LINE_PTR: usize = 0x0C8;
const BP_HDR_BOOT_FLAG: usize = 0x1FE;
const BP_HDR_HEADER: usize = 0x202;
const BP_HDR_VERSION: usize = 0x206;
const BP_HDR_TYPE_OF_LOADER: usize = 0x210;
const BP_HDR_RAMDISK_IMAGE: usize = 0x218;
const BP_HDR_RAMDISK_SIZE: usize = 0x21C;
const BP_HDR_CMD_LINE_PTR: usize = 0x228;
const BP_HDR_CMDLINE_SIZE: usize = 0x238;
const BP_E820_TABLE_OFF: usize = 0x2D0;
const BP_E820_ENTRY_SIZE: usize = 20;
const BP_E820_MAX: u8 = 128;
const BP_TYPE_OF_LOADER_UNDEFINED: u8 = 0xFF;
const BP_BOOT_FLAG: u16 = 0xAA55;
const BP_HEADER_MAGIC: u32 = 0x53726448;
const BP_VERSION_2_15: u16 = 0x020F;

// --- MP table ------------------------------------------------------------
const MP_CONFIG_ADDR: u32 = EBDA_START as u32 + 16;
const MP_OEM_ID: &[u8; 8] = b"INGOT   ";
const MP_PRODUCT_ID: &[u8; 12] = b"VMM         ";
const MP_CFG_LEN_OFF: usize = 4;
const MP_CFG_CHECKSUM_OFF: usize = 7;
const MP_CFG_ENTRIES_OFF: usize = 34;

// --- vCPU bringup --------------------------------------------------------
const CR0_PE: u64 = 1 << 0;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
/// MSR_IA32_APIC_BASE: xAPIC at default address, globally enabled, BSP.
/// Leaving it 0 would disable the in-kernel LAPIC.
const APIC_BASE_BSP_ENABLED: u64 = 0xfee0_0000 | 0x800 | 0x100;

// --- Initramfs -----------------------------------------------------------
const PAGE_MASK: u64 = 0xFFF;

struct Kernel { entry: u64, end: u64 }
struct Initramfs { start: u64, size: u64 }

pub(crate) struct BootImage<'m> {
    mem: &'m mut Memory,
    kernel_path: &'m Path,
    initramfs_path: Option<&'m Path>,
    cmdline: &'m str,
    vcpus: u8,
    mem_size: u64,
    kernel: Option<Kernel>,
    initramfs: Option<Initramfs>,
}

impl<'m> BootImage<'m> {
    pub fn new(
        mem: &'m mut Memory,
        kernel_path: &'m Path,
        initramfs_path: Option<&'m Path>,
        cmdline: &'m str,
        vcpus: u8,
        mem_size: u64,
    ) -> Self {
        Self {
            mem, kernel_path, initramfs_path, cmdline, vcpus, mem_size,
            kernel: None, initramfs: None,
        }
    }

    pub fn install(mut self) -> Result<RegState, HostError> {
        self.load_kernel()?;
        self.load_initramfs()?;
        let pt = self.build_page_tables();
        self.write(PML4_ADDR, &pt)?;
        let gdt = self.build_gdt();
        self.write(GDT_ADDR, &gdt)?;
        let cmd = self.build_cmdline();
        self.write(CMDLINE_ADDR, &cmd)?;
        let bp = self.build_boot_params()?;
        self.write(BOOT_PARAMS_ADDR, &bp)?;
        let mp = self.build_mp_table();
        self.write(EBDA_START, &mp)?;
        Ok(self.reg_state())
    }

    // --- file loaders ----------------------------------------------------

    fn load_kernel(&mut self) -> Result<(), HostError> {
        let bytes = read_file(self.kernel_path, HostError::KernelImageNotReadable)?;
        self.kernel = Some(self.load_elf(&bytes)?);
        Ok(())
    }

    fn load_initramfs(&mut self) -> Result<(), HostError> {
        let Some(path) = self.initramfs_path else { return Ok(()); };
        let bytes = read_file(path, HostError::InitramfsNotReadable)?;
        let kernel = self.kernel.as_ref().expect("load_kernel runs first");
        let start = (kernel.end + PAGE_MASK) & !PAGE_MASK;
        let size = bytes.len() as u64;
        if start.checked_add(size).is_none_or(|end| end > self.mem.size()) {
            return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
        }
        self.write(start, &bytes)?;
        self.initramfs = Some(Initramfs { start, size });
        Ok(())
    }

    fn load_elf(&mut self, image: &[u8]) -> Result<Kernel, HostError> {
        validate_elf_header(image)?;
        let (e_entry, e_phoff, e_phnum) = elf_header_fields(image);
        let mut end = 0u64;
        for i in 0..e_phnum {
            let phdr = parse_phdr(image, e_phoff + i * PHDR_SIZE)?;
            if phdr.p_type != PT_LOAD { continue; }
            if phdr.p_offset + phdr.p_filesz > image.len() {
                return Err(HostError::InvalidConfig("ELF: PT_LOAD segment past file end"));
            }
            let segment = &image[phdr.p_offset..phdr.p_offset + phdr.p_filesz];
            self.write(phdr.p_paddr, segment)?;
            end = end.max(phdr.p_paddr + phdr.p_memsz);
        }
        Ok(Kernel { entry: e_entry, end })
    }

    // --- byte builders ---------------------------------------------------

    fn build_page_tables(&self) -> [u8; PT_BUFFER_SIZE] {
        let mut buf = [0u8; PT_BUFFER_SIZE];
        put_u64(&mut buf, 0, PDPT_ADDR | PT_PRESENT | PT_RW);
        let pdpt_off = (PDPT_ADDR - PML4_ADDR) as usize;
        for i in 0..4 {
            let phys = i as u64 * PAGE_1GIB;
            put_u64(&mut buf, pdpt_off + i * 8, phys | PT_PRESENT | PT_RW | PT_PAGE_SIZE);
        }
        buf
    }

    fn build_gdt(&self) -> [u8; 24] {
        let mut buf = [0u8; 24];
        put_u64(&mut buf, 0,  NULL_DESCRIPTOR);
        put_u64(&mut buf, 8,  CODE_64BIT);
        put_u64(&mut buf, 16, DATA_64BIT);
        buf
    }

    fn build_cmdline(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(self.cmdline.len() + 1);
        buf.extend_from_slice(self.cmdline.as_bytes());
        buf.push(0);
        buf
    }

    fn build_boot_params(&self) -> Result<[u8; BP_SIZE], HostError> {
        let mut buf = [0u8; BP_SIZE];
        buf[BP_HDR_TYPE_OF_LOADER] = BP_TYPE_OF_LOADER_UNDEFINED;
        put_u16(&mut buf, BP_HDR_BOOT_FLAG, BP_BOOT_FLAG);
        put_u32(&mut buf, BP_HDR_HEADER,    BP_HEADER_MAGIC);
        put_u16(&mut buf, BP_HDR_VERSION,   BP_VERSION_2_15);
        put_u32(&mut buf, BP_HDR_CMD_LINE_PTR, CMDLINE_ADDR as u32);
        put_u32(&mut buf, BP_EXT_CMD_LINE_PTR, (CMDLINE_ADDR >> 32) as u32);
        put_u32(&mut buf, BP_HDR_CMDLINE_SIZE, CMDLINE_MAX as u32);
        if let Some(img) = &self.initramfs {
            put_u32(&mut buf, BP_HDR_RAMDISK_IMAGE, img.start as u32);
            put_u32(&mut buf, BP_EXT_RAMDISK_IMAGE, (img.start >> 32) as u32);
            put_u32(&mut buf, BP_HDR_RAMDISK_SIZE,  img.size as u32);
            put_u32(&mut buf, BP_EXT_RAMDISK_SIZE,  0);
        }
        let mut count: u8 = 0;
        for entry in e820_entries(self.mem_size)? {
            if count >= BP_E820_MAX {
                return Err(HostError::InvalidConfig("too many e820 entries"));
            }
            let off = BP_E820_TABLE_OFF + (count as usize) * BP_E820_ENTRY_SIZE;
            put_u64(&mut buf, off,      entry.addr);
            put_u64(&mut buf, off + 8,  entry.size);
            put_u32(&mut buf, off + 16, entry.kind);
            count += 1;
        }
        buf[BP_E820_ENTRIES_OFF] = count;
        Ok(buf)
    }

    fn build_mp_table(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256);
        append_mp_floating_pointer(&mut buf);
        let cfg = buf.len();
        append_mp_config_header(&mut buf);
        let mut entries: u16 = 0;
        for i in 0..self.vcpus { append_mp_processor(&mut buf, i); entries += 1; }
        append_mp_bus(&mut buf); entries += 1;
        append_mp_ioapic(&mut buf); entries += 1;
        finalize_mp_config(&mut buf, cfg, entries);
        buf
    }

    // --- writer + reg state ---------------------------------------------

    fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<(), HostError> {
        self.mem.write(addr, bytes)
            .map_err(|_| HostError::InvalidConfig("boot image write failed"))
    }

    fn reg_state(&self) -> RegState {
        let kernel = self.kernel.as_ref().expect("load_kernel runs first");
        RegState {
            regs:  build_regs(kernel.entry, BOOT_PARAMS_ADDR),
            sregs: build_sregs(),
        }
    }
}

// --- module-private free helpers (pure byte arithmetic) -----------------

fn read_file(path: &Path, err: fn(PathBuf) -> HostError) -> Result<Vec<u8>, HostError> {
    std::fs::read(path).map_err(|_| err(path.to_path_buf()))
}

fn put_u8(buf: &mut [u8], off: usize, v: u8)   { buf[off] = v; }
fn put_u16(buf: &mut [u8], off: usize, v: u16) { buf[off..off + 2].copy_from_slice(&v.to_le_bytes()); }
fn put_u32(buf: &mut [u8], off: usize, v: u32) { buf[off..off + 4].copy_from_slice(&v.to_le_bytes()); }
fn put_u64(buf: &mut [u8], off: usize, v: u64) { buf[off..off + 8].copy_from_slice(&v.to_le_bytes()); }

// --- ELF parsing helpers -------------------------------------------------

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

// --- MP table builders ---------------------------------------------------

fn append_mp_floating_pointer(buf: &mut Vec<u8>) {
    let start = buf.len();
    buf.extend_from_slice(b"_MP_");
    buf.extend_from_slice(&MP_CONFIG_ADDR.to_le_bytes());
    buf.extend_from_slice(&[1, 4, 0, 0, 0, 0, 0, 0]);
    buf[start + 10] = mp_checksum(&buf[start..start + 16]);
}

fn append_mp_config_header(buf: &mut Vec<u8>) {
    buf.extend_from_slice(b"PCMP");
    buf.extend_from_slice(&[0, 0, 4, 0]);
    buf.extend_from_slice(MP_OEM_ID);
    buf.extend_from_slice(MP_PRODUCT_ID);
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes());
    buf.extend_from_slice(&LAPIC_ADDR.to_le_bytes());
    buf.extend_from_slice(&[0, 0, 0, 0]);
}

fn append_mp_processor(buf: &mut Vec<u8>, apic_id: u8) {
    buf.extend_from_slice(&[0, apic_id, 0x14, if apic_id == 0 { 0x03 } else { 0x01 }]);
    buf.extend_from_slice(&0x600u32.to_le_bytes());
    buf.extend_from_slice(&0x0fab_fbffu32.to_le_bytes());
    buf.extend_from_slice(&[0u8; 8]);
}

fn append_mp_bus(buf: &mut Vec<u8>) {
    buf.push(1);
    buf.push(0);
    buf.extend_from_slice(b"ISA   ");
}

fn append_mp_ioapic(buf: &mut Vec<u8>) {
    buf.extend_from_slice(&[2, 0, 0x11, 0x01]);
    buf.extend_from_slice(&IOAPIC_ADDR.to_le_bytes());
}

fn finalize_mp_config(buf: &mut [u8], cfg: usize, entries: u16) {
    let cfg_len = (buf.len() - cfg) as u16;
    put_u16(buf, cfg + MP_CFG_LEN_OFF, cfg_len);
    put_u16(buf, cfg + MP_CFG_ENTRIES_OFF, entries);
    buf[cfg + MP_CFG_CHECKSUM_OFF] = mp_checksum(&buf[cfg..]);
}

fn mp_checksum(bytes: &[u8]) -> u8 {
    let s: u32 = bytes.iter().map(|&b| b as u32).sum();
    (256u32.wrapping_sub(s) & 0xff) as u8
}

// --- vCPU register state -------------------------------------------------

fn build_regs(entry: u64, params_addr: u64) -> kvm_regs {
    let mut r: kvm_regs = unsafe { std::mem::zeroed() };
    r.rip = entry;
    r.rsi = params_addr;
    r.rflags = 0x2;
    r
}

fn build_sregs() -> kvm_sregs {
    let mut s: kvm_sregs = unsafe { std::mem::zeroed() };
    s.cs = code_segment(CODE_SELECTOR);
    s.ds = data_segment(DATA_SELECTOR);
    s.es = data_segment(DATA_SELECTOR);
    s.fs = data_segment(DATA_SELECTOR);
    s.gs = data_segment(DATA_SELECTOR);
    s.ss = data_segment(DATA_SELECTOR);
    s.tr.limit = 0xFFFF;
    s.tr.type_ = 0xB;
    s.tr.present = 1;
    s.ldt.unusable = 1;
    s.gdt.base = GDT_ADDR;
    s.gdt.limit = GDT_LIMIT;
    s.cr0 = CR0_PE | CR0_NE | CR0_PG;
    s.cr3 = PML4_ADDR;
    s.cr4 = CR4_PAE;
    s.efer = EFER_LME | EFER_LMA;
    s.apic_base = APIC_BASE_BSP_ENABLED;
    s
}

fn code_segment(selector: u16) -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF;
    s.selector = selector;
    s.type_ = 0xA;
    s.present = 1;
    s.s = 1;
    s.l = 1;
    s.g = 1;
    s
}

fn data_segment(selector: u16) -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF;
    s.selector = selector;
    s.type_ = 0x2;
    s.present = 1;
    s.s = 1;
    s.db = 1;
    s.g = 1;
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::layout::KERNEL_BASE;

    const MEM_SIZE: u64 = 128 * 1024 * 1024;

    fn fresh<'m>(mem: &'m mut Memory, path: &'m Path) -> BootImage<'m> {
        BootImage::new(mem, path, None, "console=ttyS0", 1, MEM_SIZE)
    }

    fn dummy_path() -> &'static Path { Path::new("/dev/null") }

    fn synth_elf(p_paddr: u64, payload: &[u8], p_memsz: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + 56 + payload.len());
        b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&62u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&p_paddr.to_le_bytes());
        b.extend_from_slice(&64u64.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&64u16.to_le_bytes());
        b.extend_from_slice(&56u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&5u32.to_le_bytes());
        b.extend_from_slice(&(64u64 + 56).to_le_bytes());
        b.extend_from_slice(&p_paddr.to_le_bytes());
        b.extend_from_slice(&p_paddr.to_le_bytes());
        b.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        b.extend_from_slice(&p_memsz.to_le_bytes());
        b.extend_from_slice(&0x1000u64.to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn gdt_has_long_mode_code_and_data_descriptors() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let img = fresh(&mut mem, dummy_path());
        let buf = img.build_gdt();
        assert_eq!(u64::from_le_bytes(buf[0..8].try_into().unwrap()), 0);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), CODE_64BIT);
        assert_eq!(u64::from_le_bytes(buf[16..24].try_into().unwrap()), DATA_64BIT);
    }

    #[test]
    fn page_tables_identity_map_first_four_gib() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let img = fresh(&mut mem, dummy_path());
        let buf = img.build_page_tables();
        let pml4_entry = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        assert_eq!(pml4_entry & !0xFFF, PDPT_ADDR);
        let pdpt_off = (PDPT_ADDR - PML4_ADDR) as usize;
        for i in 0..4 {
            let e = u64::from_le_bytes(buf[pdpt_off + i*8..pdpt_off + i*8 + 8].try_into().unwrap());
            assert_eq!(e & !0x3FFF_FFFF, (i as u64) * PAGE_1GIB);
            assert_eq!(e & 0x83, 0x83);
        }
    }

    #[test]
    fn cmdline_bytes_followed_by_nul() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let img = BootImage::new(&mut mem, dummy_path(), None, "console=ttyS0", 1, MEM_SIZE);
        let buf = img.build_cmdline();
        assert_eq!(&buf[..buf.len() - 1], b"console=ttyS0");
        assert_eq!(*buf.last().unwrap(), 0);
    }

    #[test]
    fn boot_params_protocol_signature_and_cmdline_pointer() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let img = fresh(&mut mem, dummy_path());
        let buf = img.build_boot_params().unwrap();
        assert_eq!(buf[BP_HDR_TYPE_OF_LOADER], 0xFF);
        assert_eq!(u16::from_le_bytes([buf[0x1FE], buf[0x1FF]]), 0xAA55);
        assert_eq!(u32::from_le_bytes(buf[0x202..0x206].try_into().unwrap()), 0x53726448);
        assert_eq!(u32::from_le_bytes(buf[0x228..0x22C].try_into().unwrap()), CMDLINE_ADDR as u32);
        assert_eq!(buf[BP_E820_ENTRIES_OFF], 3);
    }

    #[test]
    fn mp_table_signatures_and_checksums() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let img = fresh(&mut mem, dummy_path());
        let buf = img.build_mp_table();
        assert_eq!(&buf[..4], b"_MP_");
        assert_eq!(buf[..16].iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        let cfg_start = 16;
        assert_eq!(&buf[cfg_start..cfg_start + 4], b"PCMP");
        let cfg_len = u16::from_le_bytes(buf[cfg_start + 4..cfg_start + 6].try_into().unwrap()) as usize;
        let cfg = &buf[cfg_start..cfg_start + cfg_len];
        assert_eq!(cfg.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        assert_eq!(u16::from_le_bytes(cfg[34..36].try_into().unwrap()), 3);
    }

    #[test]
    fn load_elf_writes_segment_and_returns_kernel_record() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let mut img = fresh(&mut mem, dummy_path());
        let elf = synth_elf(KERNEL_BASE, &[0xf4], 16);
        let k = img.load_elf(&elf).unwrap();
        assert_eq!(k.entry, KERNEL_BASE);
        assert_eq!(k.end, KERNEL_BASE + 16);
        let mut readback = [0u8; 16];
        img.mem.read(KERNEL_BASE, &mut readback).unwrap();
        assert_eq!(readback[0], 0xf4);
        assert!(readback[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn load_elf_rejects_bad_magic() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let mut img = fresh(&mut mem, dummy_path());
        let mut elf = synth_elf(KERNEL_BASE, &[0xf4], 1);
        elf[0] = 0x00;
        assert!(img.load_elf(&elf).is_err());
    }

    #[test]
    fn reg_state_points_at_kernel_entry_and_boot_params() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let mut img = fresh(&mut mem, dummy_path());
        img.kernel = Some(Kernel { entry: KERNEL_BASE, end: KERNEL_BASE + 0x1000 });
        let state = img.reg_state();
        assert_eq!(state.regs.rip, KERNEL_BASE);
        assert_eq!(state.regs.rsi, BOOT_PARAMS_ADDR);
        assert_ne!(state.sregs.cr0 & CR0_PG, 0);
        assert_eq!(state.sregs.cr3, PML4_ADDR);
        assert_ne!(state.sregs.efer & EFER_LME, 0);
        assert_eq!(state.sregs.cs.l, 1);
    }
}
