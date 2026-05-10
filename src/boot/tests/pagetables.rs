use super::super::pagetables::{self, PDPT_ADDR, PML4_ADDR};
use crate::guest_mem::GuestMem;

use super::MEM_SIZE;

fn read_entry(mem: &GuestMem, table: u64, idx: usize) -> u64 {
    let mut buf = [0u8; 8];
    mem.read(table + (idx as u64) * 8, &mut buf).expect("read");
    u64::from_le_bytes(buf)
}

#[test]
fn pml4_points_to_pdpt() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    pagetables::build(&mut mem).expect("build");
    let entry = read_entry(&mem, PML4_ADDR, 0);
    assert_eq!(entry & !0xFFF, PDPT_ADDR);
    assert_eq!(entry & 0x3, 0x3);
}

#[test]
fn pdpt_identity_maps_1gib_pages() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    pagetables::build(&mut mem).expect("build");
    for i in 0..4usize {
        let entry = read_entry(&mem, PDPT_ADDR, i);
        let expected_phys = (i as u64) * 0x4000_0000;
        assert_eq!(entry & !0x3FFF_FFFF, expected_phys, "PDPT[{}] phys", i);
        assert_eq!(entry & 0x83, 0x83, "PDPT[{}] flags (P+RW+PS)", i);
    }
}

#[test]
fn pdpt_covers_mmio_base() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    pagetables::build(&mut mem).expect("build");
    let entry = read_entry(&mem, PDPT_ADDR, 3);
    assert_eq!(entry & !0x3FFF_FFFF, 0xC000_0000, "MMIO at 0xD000_0000 falls in PDPT[3]");
}
