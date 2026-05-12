//! Failure mode for device operations driven by guest input. Every
//! device's `read`/`write` returns this. The orchestrator's run loop
//! treats it as a teardown reason — a guest that exits this way
//! crashed itself, not the VMM.

#[derive(Debug)]
pub enum GuestError {
    MalformedDescriptor,
    DescriptorOutOfBounds,
    DescriptorChainCycle,
    MmioOutOfRange { addr: u64 },
    UnexpectedPio { port: u16 },
    InvalidGuestPhys { addr: u64, len: usize },
}
