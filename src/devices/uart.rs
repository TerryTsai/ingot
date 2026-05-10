//! Memory-mapped 8250 UART. Emits guest-written bytes into an
//! internal buffer that the harness drains at run-end.
//!
//! Eight 1-byte registers (offsets 0–7). DLAB (bit 7 of LCR) toggles
//! the meaning of registers 0 and 1 between divisor latches and
//! THR/IER. Linux's serial driver writes a divisor it never observes
//! (we don't model real bauds), then writes characters to THR.

use super::{GuestError, Interrupt, MmioDevice};

const REG_RBR_THR_DLL: u64 = 0;
const REG_IER_DLM: u64 = 1;
const REG_IIR_FCR: u64 = 2;
const REG_LCR: u64 = 3;
const REG_MCR: u64 = 4;
const REG_LSR: u64 = 5;
const REG_MSR: u64 = 6;
const REG_SCR: u64 = 7;

const LCR_DLAB: u8 = 1 << 7;
const LSR_THRE_AND_TEMT: u8 = 0x60;
const IIR_NO_INTERRUPT: u8 = 0x01;

pub(crate) struct Uart {
    buffer: Vec<u8>,
    ier: u8,
    fcr: u8,
    lcr: u8,
    mcr: u8,
    scr: u8,
    dll: u8,
    dlm: u8,
}

impl Uart {
    pub fn new() -> Self {
        Uart { buffer: Vec::new(), ier: 0, fcr: 0, lcr: 0, mcr: 0, scr: 0, dll: 0, dlm: 0 }
    }

    pub fn take_buffer(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.buffer)
    }

    fn dlab(&self) -> bool {
        self.lcr & LCR_DLAB != 0
    }
}

impl MmioDevice for Uart {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if data.len() != 1 || offset > REG_SCR {
            return Err(GuestError::MmioOutOfRange { addr: offset });
        }
        data[0] = match offset {
            REG_RBR_THR_DLL if self.dlab() => self.dll,
            REG_RBR_THR_DLL => 0,
            REG_IER_DLM if self.dlab() => self.dlm,
            REG_IER_DLM => self.ier,
            REG_IIR_FCR => IIR_NO_INTERRUPT,
            REG_LCR => self.lcr,
            REG_MCR => self.mcr,
            REG_LSR => LSR_THRE_AND_TEMT,
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
        let value = data[0];
        match offset {
            REG_RBR_THR_DLL if self.dlab() => self.dll = value,
            REG_RBR_THR_DLL => self.buffer.push(value),
            REG_IER_DLM if self.dlab() => self.dlm = value,
            REG_IER_DLM => self.ier = value,
            REG_IIR_FCR => self.fcr = value,
            REG_LCR => self.lcr = value,
            REG_MCR => self.mcr = value,
            REG_SCR => self.scr = value,
            _ => {}
        }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        None
    }
}
