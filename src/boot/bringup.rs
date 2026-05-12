//! Phase-2 bringup-table byte assembly. Writes the GDT, boot_params,
//! page tables, cmdline, and MP table into guest memory at their
//! fixed `layout::*` addresses through the `Asm` byte assembler.
//!
//! Reads top-to-bottom as a memory map — scrolling walks guest memory
//! from low to high address. Sparse layouts (boot_params, page
//! tables) only write the fields they care about and rely on Memory
//! being zero-initialized by `MAP_ANONYMOUS`. Do not call against
//! pre-written memory without clearing it.
//!
//! No persistent state — this is a free function. The "type
//! gymnastics" of orchestrating Kernel/Initramfs/Registers happens
//! in `boot::install`, where this is one step.

use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

use super::asm::Asm;
use super::layout::{
    e820_entries, BOOT_PARAMS_ADDR, CMDLINE_ADDR, CMDLINE_MAX, EBDA_START,
    GDT_ADDR, IOAPIC_ADDR, LAPIC_ADDR, PDPT_ADDR, PML4_ADDR,
};
use super::loaders::Initramfs;

pub(super) fn write(
    mem: &mut Memory,
    cmdline: &str,
    vcpus: u8,
    initramfs: Option<&Initramfs>,
    mem_size: u64,
) -> Result<(), HostError> {
    let e820 = e820_entries(mem_size)?;
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
    if let Some(i) = initramfs {
        a.d(0x218, i.start() as u32);                   // ramdisk_image (lo)
        a.d(0x0C0, (i.start() >> 32) as u32);           // ramdisk_image (hi)
        a.d(0x21C, i.size() as u32);                    // ramdisk_size
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM_SIZE: u64 = 128 * 1024 * 1024;

    #[test]
    fn writes_gdt_descriptors() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        write(&mut mem, "", 1, None, MEM_SIZE).unwrap();
        let mut buf = [0u8; 24];
        mem.read(GDT_ADDR, &mut buf).unwrap();
        assert_eq!(u64::from_le_bytes(buf[0..8].try_into().unwrap()), 0);
        assert_eq!(u64::from_le_bytes(buf[8..16].try_into().unwrap()), 0x00AF_9A00_0000_FFFF);
        assert_eq!(u64::from_le_bytes(buf[16..24].try_into().unwrap()), 0x00CF_9200_0000_FFFF);
    }

    #[test]
    fn writes_boot_params_signature_and_cmdline_pointer() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        write(&mut mem, "ttyS0", 1, None, MEM_SIZE).unwrap();
        let mut buf = [0u8; 4096];
        mem.read(BOOT_PARAMS_ADDR, &mut buf).unwrap();
        assert_eq!(buf[0x210], 0xFF);
        assert_eq!(u16::from_le_bytes([buf[0x1FE], buf[0x1FF]]), 0xAA55);
        assert_eq!(u32::from_le_bytes(buf[0x202..0x206].try_into().unwrap()), 0x53726448);
        assert_eq!(u32::from_le_bytes(buf[0x228..0x22C].try_into().unwrap()), CMDLINE_ADDR as u32);
        assert_eq!(buf[0x1E8], 3);
    }

    #[test]
    fn writes_page_tables_identity_mapping() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        write(&mut mem, "", 1, None, MEM_SIZE).unwrap();
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
    fn writes_cmdline_string_with_nul() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        write(&mut mem, "console=ttyS0", 1, None, MEM_SIZE).unwrap();
        let mut buf = [0u8; 14];
        mem.read(CMDLINE_ADDR, &mut buf).unwrap();
        assert_eq!(&buf[..13], b"console=ttyS0");
        assert_eq!(buf[13], 0);
    }

    #[test]
    fn writes_mp_table_signatures_and_valid_checksums() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        write(&mut mem, "", 1, None, MEM_SIZE).unwrap();
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
