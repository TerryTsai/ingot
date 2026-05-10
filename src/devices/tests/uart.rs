use crate::devices::{MmioDevice, Uart};

#[test]
fn thr_writes_accumulate_in_buffer() {
    let mut uart = Uart::new();
    uart.write(0, b"h").unwrap();
    uart.write(0, b"i").unwrap();
    assert_eq!(uart.take_buffer(), b"hi".to_vec());
}

#[test]
fn take_buffer_drains() {
    let mut uart = Uart::new();
    uart.write(0, b"x").unwrap();
    let _ = uart.take_buffer();
    assert!(uart.take_buffer().is_empty());
}

#[test]
fn lsr_reports_thr_empty() {
    let mut uart = Uart::new();
    let mut buf = [0u8; 1];
    uart.read(5, &mut buf).unwrap();
    assert_eq!(buf[0] & 0x60, 0x60);
}

#[test]
fn iir_reports_no_interrupt() {
    let mut uart = Uart::new();
    let mut buf = [0u8; 1];
    uart.read(2, &mut buf).unwrap();
    assert_eq!(buf[0], 0x01);
}

#[test]
fn dlab_toggles_register_zero_one() {
    let mut uart = Uart::new();
    uart.write(3, &[0x80]).unwrap(); // LCR with DLAB=1
    uart.write(0, &[0x12]).unwrap(); // DLL
    uart.write(1, &[0x34]).unwrap(); // DLM
    uart.write(3, &[0x03]).unwrap(); // LCR with DLAB=0, 8N1
    uart.write(0, &[b'A']).unwrap(); // THR
    assert_eq!(uart.take_buffer(), b"A".to_vec());
}

#[test]
fn rejects_oversized_access() {
    let mut uart = Uart::new();
    let r = uart.write(0, &[0xAA, 0xBB]);
    assert!(r.is_err());
}

#[test]
fn rejects_offset_past_end() {
    let mut uart = Uart::new();
    let mut buf = [0u8; 1];
    let r = uart.read(8, &mut buf);
    assert!(r.is_err());
}

#[test]
fn pending_interrupt_is_none() {
    let mut uart = Uart::new();
    assert!(uart.pending_interrupt().is_none());
}
