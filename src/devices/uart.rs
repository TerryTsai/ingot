//! Memory-mapped 8250 UART. Output → caller-provided Write sink.
//! Input → caller pushes bytes through a UartInput handle, which the
//! UART surfaces via the RBR register and LSR.DR bit.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex};

use super::{GuestError, Interrupt, MmioDevice};

const REG_RBR_THR_DLL: u64 = 0;
const REG_IER_DLM: u64 = 1;
const REG_IIR_FCR: u64 = 2;
const REG_LCR: u64 = 3;
const REG_MCR: u64 = 4;
const REG_LSR: u64 = 5;
const REG_MSR: u64 = 6;
const REG_SCR: u64 = 7;

const LCR_DLAB: u8 = 0x80;
const LSR_THRE_TEMT: u8 = 0x60;
const LSR_DR: u8 = 0x01;
const IIR_NONE: u8 = 0x01;

pub(crate) struct Uart {
    sink: Box<dyn Write + Send>,
    input: Arc<Mutex<VecDeque<u8>>>,
    ier: u8, fcr: u8, lcr: u8, mcr: u8, scr: u8, dll: u8, dlm: u8,
}

pub(crate) struct UartInput(Arc<Mutex<VecDeque<u8>>>);

impl UartInput {
    pub fn push(&self, b: u8) {
        self.0.lock().unwrap().push_back(b);
    }
}

impl Uart {
    pub fn new(sink: Box<dyn Write + Send>) -> Self {
        Uart {
            sink,
            input: Arc::new(Mutex::new(VecDeque::new())),
            ier: 0, fcr: 0, lcr: 0, mcr: 0, scr: 0, dll: 0, dlm: 0,
        }
    }

    pub fn input_handle(&self) -> UartInput {
        UartInput(self.input.clone())
    }

    fn dlab(&self) -> bool { self.lcr & LCR_DLAB != 0 }

    fn lsr(&self) -> u8 {
        LSR_THRE_TEMT | if self.input.lock().unwrap().is_empty() { 0 } else { LSR_DR }
    }
}

impl MmioDevice for Uart {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if data.len() != 1 || offset > REG_SCR {
            return Err(GuestError::MmioOutOfRange { addr: offset });
        }
        data[0] = match offset {
            REG_RBR_THR_DLL if self.dlab() => self.dll,
            REG_RBR_THR_DLL => self.input.lock().unwrap().pop_front().unwrap_or(0),
            REG_IER_DLM if self.dlab() => self.dlm,
            REG_IER_DLM => self.ier,
            REG_IIR_FCR => IIR_NONE,
            REG_LCR => self.lcr,
            REG_MCR => self.mcr,
            REG_LSR => self.lsr(),
            REG_MSR => 0,
            REG_SCR => self.scr,
            _ => 0,
        };
        Ok(())
    }

    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError> {
        if data.len() != 1 || offset > REG_SCR {
            return Err(GuestError::MmioOutOfRange { addr: offset });
        }
        let v = data[0];
        match offset {
            REG_RBR_THR_DLL if self.dlab() => self.dll = v,
            REG_RBR_THR_DLL => { let _ = self.sink.write_all(&[v]); let _ = self.sink.flush(); }
            REG_IER_DLM if self.dlab() => self.dlm = v,
            REG_IER_DLM => self.ier = v,
            REG_IIR_FCR => self.fcr = v,
            REG_LCR => self.lcr = v,
            REG_MCR => self.mcr = v,
            REG_SCR => self.scr = v,
            _ => {}
        }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> { None }
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
        uart.input_handle().push(b'q');
        let mut uart = uart;
        let mut buf = [0u8; 1];
        uart.read(5, &mut buf).unwrap();
        assert_eq!(buf[0] & 0x01, 0x01);
    }

    #[test]
    fn rbr_pops_pushed_input() {
        let (sink, _) = new_capture();
        let uart = Uart::new(sink);
        uart.input_handle().push(b'a');
        uart.input_handle().push(b'b');
        let mut uart = uart;
        let mut buf = [0u8; 1];
        uart.read(0, &mut buf).unwrap();
        assert_eq!(buf[0], b'a');
        uart.read(0, &mut buf).unwrap();
        assert_eq!(buf[0], b'b');
        uart.read(0, &mut buf).unwrap();
        assert_eq!(buf[0], 0);
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
}
