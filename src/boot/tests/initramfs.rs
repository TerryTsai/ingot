use crate::boot::{load_initramfs, KernelImage};
use crate::guest_mem::GuestMem;

use super::MEM_SIZE;

#[test]
fn initramfs_placed_after_kernel_page_aligned() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let kernel = KernelImage { entry_point: 0x10_0000, end_addr: 0x10_0100 };
    let img = load_initramfs(b"hello", &mut mem, &kernel).expect("load");
    assert_eq!(img.start_addr & 0xFFF, 0, "page-aligned");
    assert!(img.start_addr >= kernel.end_addr);
    assert_eq!(img.size, 5);
    let mut buf = [0u8; 5];
    mem.read(img.start_addr, &mut buf).expect("read");
    assert_eq!(&buf, b"hello");
}

#[test]
fn rejects_initramfs_too_big_for_guest_memory() {
    let mut mem = GuestMem::new(0x10_2000).expect("mem");
    let kernel = KernelImage { entry_point: 0x10_0000, end_addr: 0x10_1000 };
    let big = vec![0u8; 0x10_0000];
    assert!(load_initramfs(&big, &mut mem, &kernel).is_err());
}
