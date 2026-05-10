use crate::boot::load_kernel;
use crate::guest_mem::GuestMem;

use super::{synth_elf, synth_elf_full, ENTRY_POINT, MEM_SIZE, SEGMENT_PADDR};

#[test]
fn loads_minimal_elf() {
    let elf = synth_elf(SEGMENT_PADDR, &[0xf4]);
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let img = load_kernel(&elf, &mut mem).expect("load");
    assert_eq!(img.entry_point, ENTRY_POINT);
    assert_eq!(img.end_addr, SEGMENT_PADDR + 1);
    let mut readback = [0u8; 1];
    mem.read(SEGMENT_PADDR, &mut readback).expect("read");
    assert_eq!(readback[0], 0xf4);
}

#[test]
fn bss_zero_filled() {
    let elf = synth_elf_full(SEGMENT_PADDR, &[0x42], 16);
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let img = load_kernel(&elf, &mut mem).expect("load");
    assert_eq!(img.end_addr, SEGMENT_PADDR + 16);
    let mut readback = [0u8; 16];
    mem.read(SEGMENT_PADDR, &mut readback).expect("read");
    assert_eq!(readback[0], 0x42);
    assert!(readback[1..].iter().all(|&b| b == 0));
}

#[test]
fn rejects_truncated_header() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(&[0x7f, b'E', b'L', b'F'], &mut mem).is_err());
}

#[test]
fn rejects_bad_magic() {
    let mut elf = synth_elf(SEGMENT_PADDR, &[0xf4]);
    elf[0] = 0x00;
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(&elf, &mut mem).is_err());
}

#[test]
fn rejects_32bit() {
    let mut elf = synth_elf(SEGMENT_PADDR, &[0xf4]);
    elf[4] = 1;
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(&elf, &mut mem).is_err());
}

#[test]
fn rejects_wrong_machine() {
    let mut elf = synth_elf(SEGMENT_PADDR, &[0xf4]);
    elf[18..20].copy_from_slice(&3u16.to_le_bytes()); // EM_386
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(&elf, &mut mem).is_err());
}

#[test]
fn rejects_segment_past_file_end() {
    let elf = synth_elf(SEGMENT_PADDR, &[0xf4]);
    let truncated = &elf[..elf.len() - 1];
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(truncated, &mut mem).is_err());
}

#[test]
fn rejects_segment_past_guest_mem() {
    let elf = synth_elf(MEM_SIZE + 0x1000, &[0xf4]);
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    assert!(load_kernel(&elf, &mut mem).is_err());
}
