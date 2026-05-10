use super::super::gdt::{self, GDT_ADDR};
use crate::guest_mem::GuestMem;

use super::MEM_SIZE;

fn descriptor(mem: &GuestMem, idx: u64) -> u64 {
    let mut buf = [0u8; 8];
    mem.read(GDT_ADDR + idx * 8, &mut buf).expect("read");
    u64::from_le_bytes(buf)
}

#[test]
fn null_descriptor_is_zero() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    gdt::build(&mut mem).expect("build");
    assert_eq!(descriptor(&mem, 0), 0);
}

#[test]
fn code_descriptor_is_long_mode() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    gdt::build(&mut mem).expect("build");
    let cs = descriptor(&mem, 1);
    assert_ne!(cs & (1 << 53), 0, "L bit (long mode)");
    assert_ne!(cs & (1 << 47), 0, "P bit (present)");
    assert_ne!(cs & (1 << 43), 0, "executable type");
    assert_ne!(cs & (1 << 44), 0, "S bit (code/data, not system)");
}

#[test]
fn data_descriptor_present() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    gdt::build(&mut mem).expect("build");
    let ds = descriptor(&mem, 2);
    assert_ne!(ds & (1 << 47), 0, "P bit");
    assert_ne!(ds & (1 << 44), 0, "S bit");
    assert_eq!(ds & (1 << 43), 0, "type bit clear (data, not code)");
}
