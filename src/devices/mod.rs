//! virtio devices and 8250 UART. MMIO transport only.
//!
//! Phases 2-5 deliverable. Currently a sketch.

mod block;
mod net;
mod uart;
mod vsock;

pub(crate) use block::Block;
pub(crate) use net::Net;
pub(crate) use uart::Uart;
pub(crate) use vsock::Vsock;

pub(crate) trait MmioDevice: Send {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError>;
    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError>;
    fn pending_interrupt(&mut self) -> Option<Interrupt>;
}

pub(crate) struct Interrupt {
    pub gsi: u32,
}

pub(crate) struct MmioBus {
    _placeholder: (),
}

impl MmioBus {
    pub fn new() -> Self {
        unimplemented!()
    }

    pub fn read(&mut self, addr: u64, data: &mut [u8]) -> Result<(), GuestError> {
        unimplemented!()
    }

    pub fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), GuestError> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub enum GuestError {
    MalformedDescriptor,
    DescriptorOutOfBounds,
    DescriptorChainCycle,
    MmioOutOfRange { addr: u64 },
    UnexpectedPio { port: u16 },
    InvalidGuestPhys { addr: u64, len: usize },
}
