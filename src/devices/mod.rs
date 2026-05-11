//! MMIO devices: the trait every device implements, the bus that
//! dispatches to them, and the 8250 UART. virtio block/net/vsock are
//! Phase 3+ placeholders.

mod block;
mod mmio_bus;
mod net;
mod uart;
mod vsock;

pub(crate) use mmio_bus::MmioBus;
pub(crate) use uart::{Uart, UartInput};

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
