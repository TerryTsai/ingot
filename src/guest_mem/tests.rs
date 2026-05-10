use super::GuestMem;

const ONE_MB: u64 = 1024 * 1024;

#[test]
fn allocates_and_writes() {
    let mut mem = GuestMem::new(ONE_MB).expect("mmap");
    mem.write(0, b"hello").expect("write");
    let mut buf = [0u8; 5];
    mem.read(0, &mut buf).expect("read");
    assert_eq!(&buf, b"hello");
}

#[test]
fn rejects_out_of_bounds_write() {
    let mut mem = GuestMem::new(ONE_MB).expect("mmap");
    let r = mem.write(ONE_MB - 4, &[1, 2, 3, 4, 5]);
    assert!(r.is_err());
}

#[test]
fn rejects_overflow_in_bounds_check() {
    let mut mem = GuestMem::new(ONE_MB).expect("mmap");
    let r = mem.write(u64::MAX - 1, &[1, 2, 3]);
    assert!(r.is_err());
}

#[test]
fn write_at_boundary_succeeds() {
    let mut mem = GuestMem::new(ONE_MB).expect("mmap");
    let buf = vec![0xab; 16];
    mem.write(ONE_MB - 16, &buf).expect("write at boundary");
    let mut readback = [0u8; 16];
    mem.read(ONE_MB - 16, &mut readback).expect("read at boundary");
    assert_eq!(readback.as_slice(), buf.as_slice());
}
