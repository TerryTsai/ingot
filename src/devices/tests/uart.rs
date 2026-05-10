use crate::devices::{MmioDevice, Uart};

use super::new_capture;

#[test]
fn thr_writes_go_to_sink() {
    let (sink, captured) = new_capture();
    let mut uart = Uart::new(sink);
    uart.write(0, b"h").unwrap();
    uart.write(0, b"i").unwrap();
    assert_eq!(*captured.lock().unwrap(), b"hi".to_vec());
}

#[test]
fn lsr_reports_thr_empty() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    let mut buf = [0u8; 1];
    uart.read(5, &mut buf).unwrap();
    assert_eq!(buf[0] & 0x60, 0x60);
}

#[test]
fn iir_reports_no_interrupt() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    let mut buf = [0u8; 1];
    uart.read(2, &mut buf).unwrap();
    assert_eq!(buf[0], 0x01);
}

#[test]
fn dlab_toggles_register_zero_one() {
    let (sink, captured) = new_capture();
    let mut uart = Uart::new(sink);
    uart.write(3, &[0x80]).unwrap(); // LCR DLAB=1
    uart.write(0, &[0x12]).unwrap(); // DLL — not THR
    uart.write(1, &[0x34]).unwrap(); // DLM — not IER
    uart.write(3, &[0x03]).unwrap(); // LCR DLAB=0
    uart.write(0, &[b'A']).unwrap(); // THR
    assert_eq!(*captured.lock().unwrap(), b"A".to_vec());
}

#[test]
fn rejects_oversized_access() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    let r = uart.write(0, &[0xAA, 0xBB]);
    assert!(r.is_err());
}

#[test]
fn rejects_offset_past_end() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    let mut buf = [0u8; 1];
    let r = uart.read(8, &mut buf);
    assert!(r.is_err());
}

#[test]
fn pending_interrupt_is_none() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    assert!(uart.pending_interrupt().is_none());
}
