//! Devices: virtio block/net/vsock over the MMIO bus, plus the legacy
//! 8250 UART exposed at PIO COM1 (§3.1.3). One primitive per file —
//! struct + every method on it live together.

mod block;
mod mmio_bus;
mod net;
mod tap;
mod uart;
mod vhost_vsock;
mod virtio_mmio;
mod virtio_queue;
mod vsock;

pub(crate) use block::Block;
pub(crate) use mmio_bus::MmioBus;
pub(crate) use net::Net;
pub(crate) use tap::Tap;
pub(crate) use uart::{Uart, UartInput, COM1_BASE, COM1_GSI, COM1_LAST};
pub(crate) use vsock::Vsock;

pub(crate) trait MmioDevice: Send {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError>;
    fn write(&mut self, offset: u64, data: &[u8], mem: &mut crate::memory::Memory) -> Result<(), GuestError>;
    fn pending_interrupt(&mut self) -> Option<Interrupt>;
    /// Called after each vCPU exit. Devices with async I/O (TAP, AF_VSOCK)
    /// drain their host-side fd here and stage RX into their virtqueues.
    /// Default: no-op for synchronous-only devices.
    fn poll_io(&mut self, _mem: &mut crate::memory::Memory) -> Result<(), GuestError> { Ok(()) }
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
