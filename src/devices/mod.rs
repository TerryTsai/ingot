//! virtio devices and 8250 UART. MMIO transport only.
//!
//! UART and MmioBus are real (Phase 2). The virtio devices remain
//! sketches until Phases 3–5.

mod block;
mod bus;
mod net;
mod uart;
mod vsock;

#[cfg(test)]
mod tests;

pub(crate) use bus::MmioBus;
pub(crate) use uart::{Uart, UartInput};
// block::Block, net::Net, vsock::Vsock re-exports added in Phase 3+ when used.

pub(crate) trait MmioDevice: Send {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError>;
    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError>;
    fn pending_interrupt(&mut self) -> Option<Interrupt>;
}

pub(crate) struct Interrupt {
    pub gsi: u32,
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
