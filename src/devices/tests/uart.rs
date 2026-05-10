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
fn lsr_reports_data_ready_when_input_pending() {
    let (sink, _) = new_capture();
    let uart = Uart::new(sink);
    let input = uart.input_handle();
    input.push(b'q');
    let mut uart = uart;
    let mut buf = [0u8; 1];
    uart.read(5, &mut buf).unwrap();
    assert_eq!(buf[0] & 0x01, 0x01, "DR bit set");
}

#[test]
fn rbr_pops_pushed_input() {
    let (sink, _) = new_capture();
    let uart = Uart::new(sink);
    let input = uart.input_handle();
    input.push(b'a');
    input.push(b'b');
    let mut uart = uart;
    let mut buf = [0u8; 1];
    uart.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], b'a');
    uart.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], b'b');
    uart.read(0, &mut buf).unwrap();
    assert_eq!(buf[0], 0, "empty queue reads as 0");
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
    uart.write(3, &[0x80]).unwrap();
    uart.write(0, &[0x12]).unwrap();
    uart.write(1, &[0x34]).unwrap();
    uart.write(3, &[0x03]).unwrap();
    uart.write(0, &[b'A']).unwrap();
    assert_eq!(*captured.lock().unwrap(), b"A".to_vec());
}

#[test]
fn rejects_oversized_access() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    assert!(uart.write(0, &[0xAA, 0xBB]).is_err());
}

#[test]
fn rejects_offset_past_end() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    let mut buf = [0u8; 1];
    assert!(uart.read(8, &mut buf).is_err());
}

#[test]
fn pending_interrupt_is_none() {
    let (sink, _) = new_capture();
    let mut uart = Uart::new(sink);
    assert!(uart.pending_interrupt().is_none());
}
