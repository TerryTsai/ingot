//! Devices — the fake hardware the guest sees. Four top-level concerns:
//!
//!   bus.rs        Routes a guest read/write (MMIO or PIO) to whichever
//!                 device claims that address.
//!
//!   uart.rs       Standalone legacy device — 8250 serial console
//!                 (PIO 0x3F8, IRQ 4). Writes guest output to stdout.
//!
//!   virtio/       The virtio family. Three devices (net, block, vsock)
//!                 sharing register layout (core/mmio.rs) and queue
//!                 mechanism (core/queue.rs).
//!
//!   host/         Wrappers around host-side facilities (TAP fd,
//!                 vhost-vsock fd) that the virtio devices use to
//!                 actually do I/O.
//!
//! Address map (x86_64):
//!   PIO  0x3F8           UART          IRQ 4
//!   MMIO 0xD000_0000     virtio-net    IRQ 5
//!   MMIO 0xD000_1000     virtio-block  IRQ 6
//!   MMIO 0xD000_2000     virtio-vsock  IRQ 7

mod bus;
mod cmos;
mod host;
mod uart;
mod virtio;

pub(crate) use bus::{Bus, MmioDevice};
pub(crate) use cmos::Cmos;
pub(crate) use host::Tap;
pub(crate) use uart::{Uart, UartInput, COM1_BASE, COM1_SIZE};
pub(crate) use virtio::{Block, Net, Vsock};

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
