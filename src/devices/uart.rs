//! 8250 UART on COM1 (PIO 0x3F8). The 8250 register layout lets Linux's
//! 8250 driver auto-detect COM1 without DT/ACPI plumbing. The kernel uses
//! IRQ 4 for COM1's tty path; polling-mode printk works without IRQs, but
//! user-space writes do not. Implements `PioDevice` so the bus routes
//! port 0x3F8..0x3FF accesses to this device.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

use super::bus::PioDevice;
use super::Interrupt;

// 8250 register offsets (relative to COM1_BASE)
const REG_RBR_THR_DLL: u16 = 0;
const REG_IER_DLM: u16 = 1;
const REG_IIR_FCR: u16 = 2;
const REG_LCR: u16 = 3;
const REG_MCR: u16 = 4;
const REG_LSR: u16 = 5;
const REG_SCR: u16 = 7;

const LCR_DLAB: u8 = 0x80;
const LSR_READY_TX: u8 = 0x60;
const LSR_READY_RX: u8 = 0x01;
const IIR_NONE: u8 = 0x01;
const IIR_THR_EMPTY: u8 = 0x02;
const IIR_RX_DATA: u8 = 0x04;
const IER_RX: u8 = 0x01;
const IER_TX: u8 = 0x02;

pub(crate) const COM1_BASE: u16 = 0x3F8;
pub(crate) const COM1_SIZE: u16 = 8;
pub(crate) const COM1_GSI: u32 = 4;

/// `rx_armed` gates RBR access until the driver writes IER with RX-
/// enable; 8250 startup reads RBR to clear stuck state and without the
/// gate it silently consumes bytes our stdin thread had already queued.
pub(crate) struct Uart {
    sink: Box<dyn Write + Send>,
    input: Arc<Mutex<VecDeque<u8>>>,
    ier: u8, fcr: u8, lcr: u8, mcr: u8, scr: u8, dll: u8, dlm: u8,
    rx_armed: bool,
}

pub(crate) struct UartInput(Arc<Mutex<VecDeque<u8>>>);

impl UartInput {
    pub fn push(&self, b: u8) { self.0.lock().unwrap().push_back(b); }
}

impl Uart {
    pub fn new(sink: Box<dyn Write + Send>) -> Self {
        Uart { sink, input: Arc::new(Mutex::new(VecDeque::new())),
            ier: 0, fcr: 0, lcr: 0, mcr: 0, scr: 0, dll: 0, dlm: 0, rx_armed: false }
    }

    pub fn input_handle(&self) -> UartInput { UartInput(self.input.clone()) }

    fn read_register(&mut self, reg: u16) -> u8 {
        match reg {
            REG_RBR_THR_DLL if self.dlab() => self.dll,
            REG_RBR_THR_DLL if self.rx_armed => self.input.lock().unwrap().pop_front().unwrap_or(0),
            REG_RBR_THR_DLL => 0,
            REG_IER_DLM if self.dlab() => self.dlm,
            REG_IER_DLM => self.ier,
            REG_IIR_FCR => self.iir(),
            REG_LCR => self.lcr,
            REG_MCR => self.mcr,
            REG_LSR => self.lsr(),
            REG_SCR => self.scr,
            _ => 0,
        }
    }

    fn write_register(&mut self, reg: u16, v: u8) {
        match reg {
            REG_RBR_THR_DLL if self.dlab() => self.dll = v,
            REG_RBR_THR_DLL => { let _ = self.sink.write_all(&[v]); let _ = self.sink.flush(); }
            REG_IER_DLM if self.dlab() => self.dlm = v,
            REG_IER_DLM => { self.ier = v & 0x0F; self.rx_armed |= self.ier & IER_RX != 0; }
            REG_IIR_FCR => self.fcr = v,
            REG_LCR => self.lcr = v,
            REG_MCR => self.mcr = v,
            REG_SCR => self.scr = v,
            _ => {}
        }
    }

    fn dlab(&self) -> bool { self.lcr & LCR_DLAB != 0 }

    fn lsr(&self) -> u8 {
        let dr = self.rx_armed && !self.input.lock().unwrap().is_empty();
        LSR_READY_TX | if dr { LSR_READY_RX } else { 0 }
    }

    fn iir(&self) -> u8 {
        if self.ier & IER_RX != 0 && !self.input.lock().unwrap().is_empty() { IIR_RX_DATA }
        else if self.ier & IER_TX != 0 { IIR_THR_EMPTY }
        else { IIR_NONE }
    }
}

impl PioDevice for Uart {
    fn read(&mut self, port_offset: u16, data: &mut [u8]) {
        let v = self.read_register(port_offset);
        for b in data.iter_mut() { *b = v; }
    }

    fn write(&mut self, port_offset: u16, data: &[u8]) {
        if let Some(&v) = data.first() {
            self.write_register(port_offset, v);
        }
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.iir() != IIR_NONE { Some(Interrupt { gsi: COM1_GSI }) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl io::Write for Captured {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> { Ok(()) }
    }

    fn new_capture() -> (Box<dyn Write + Send>, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (Box::new(Captured(buf.clone())), buf)
    }

    fn read_byte(uart: &mut Uart, off: u16) -> u8 {
        let mut b = [0u8; 1];
        uart.read(off, &mut b);
        b[0]
    }

    #[test]
    fn thr_writes_go_to_sink() {
        let (sink, captured) = new_capture();
        let mut uart = Uart::new(sink);
        uart.write(REG_RBR_THR_DLL, &[b'h']);
        uart.write(REG_RBR_THR_DLL, &[b'i']);
        assert_eq!(*captured.lock().unwrap(), b"hi".to_vec());
    }

    #[test]
    fn lsr_reports_thr_empty() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        assert_eq!(read_byte(&mut uart, REG_LSR) & 0x60, 0x60);
    }

    #[test]
    fn lsr_reports_data_ready_only_after_driver_arms_rx() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        uart.input_handle().push(b'q');
        assert_eq!(read_byte(&mut uart, REG_LSR) & 0x01, 0, "DR clear pre-arm");
        uart.write(REG_IER_DLM, &[IER_RX]);
        assert_eq!(read_byte(&mut uart, REG_LSR) & 0x01, 0x01, "DR set once armed");
    }

    #[test]
    fn rbr_does_not_pop_until_driver_arms_rx() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        uart.input_handle().push(b'a');
        uart.input_handle().push(b'b');
        assert_eq!(read_byte(&mut uart, REG_RBR_THR_DLL), 0);
        assert_eq!(read_byte(&mut uart, REG_RBR_THR_DLL), 0);
        uart.write(REG_IER_DLM, &[IER_RX]);
        assert_eq!(read_byte(&mut uart, REG_RBR_THR_DLL), b'a');
        assert_eq!(read_byte(&mut uart, REG_RBR_THR_DLL), b'b');
        assert_eq!(read_byte(&mut uart, REG_RBR_THR_DLL), 0);
    }

    #[test]
    fn iir_reports_no_interrupt_when_disabled() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        assert_eq!(read_byte(&mut uart, REG_IIR_FCR), IIR_NONE);
        assert!(uart.pending_interrupt().is_none());
    }

    #[test]
    fn iir_reports_thr_empty_when_tx_enabled() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        uart.write(REG_IER_DLM, &[IER_TX]);
        assert_eq!(read_byte(&mut uart, REG_IIR_FCR), IIR_THR_EMPTY);
        assert!(uart.pending_interrupt().is_some());
    }

    #[test]
    fn iir_reports_rx_data_when_rx_enabled_and_input_pending() {
        let (sink, _) = new_capture();
        let mut uart = Uart::new(sink);
        uart.write(REG_IER_DLM, &[IER_RX]);
        uart.input_handle().push(b'x');
        assert_eq!(read_byte(&mut uart, REG_IIR_FCR), IIR_RX_DATA);
        let intr = uart.pending_interrupt().unwrap();
        assert_eq!(intr.gsi, COM1_GSI);
    }
}
