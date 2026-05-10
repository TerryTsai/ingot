use super::super::pagetables::{self, PD_ADDR, PDPT_ADDR, PML4_ADDR};
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
fn pdpt_points_to_pd() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    pagetables::build(&mut mem).expect("build");
    let entry = read_entry(&mem, PDPT_ADDR, 0);
    assert_eq!(entry & !0xFFF, PD_ADDR);
    assert_eq!(entry & 0x3, 0x3);
}

#[test]
fn pd_identity_maps_2mb_pages() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    pagetables::build(&mut mem).expect("build");
    for i in 0..512usize {
        let entry = read_entry(&mem, PD_ADDR, i);
        let expected_phys = (i as u64) * 0x20_0000;
        assert_eq!(entry & !0x1F_FFFF, expected_phys, "PD[{}] phys", i);
        assert_eq!(entry & 0x83, 0x83, "PD[{}] flags", i);
    }
}
