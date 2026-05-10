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
        let pending = !self.input.lock().unwrap().is_empty();
        LSR_THRE_TEMT | if pending { LSR_DR } else { 0 }
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
