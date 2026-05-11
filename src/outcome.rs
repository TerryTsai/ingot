use std::time::Instant;

use crate::state::State;

pub struct Outcome {
    pub exit_status: ExitStatus,
    pub telemetry: Telemetry,
    pub stdout: Vec<u8>,
}

#[derive(Debug)]
pub enum ExitStatus {
    Halted,
    KernelPanic,
    GuestFault(GuestFaultReason),
    ResourceExhausted,
}

#[derive(Debug)]
pub enum GuestFaultReason {
    MalformedDescriptor,
    DescriptorOutOfBounds,
    DescriptorChainCycle,
    UnexpectedPio,
    MmioOutOfRange,
    /// CPU shut down. On x86_64 this is almost always a triple fault —
    /// the guest's exception handlers themselves faulted, so the CPU
    /// gave up. Common early-boot causes: no IDT (so any #PF/#GP cascades),
    /// stack misalignment, invalid MSR write.
    TripleFault,
}

pub struct Telemetry {
    pub boot_started_at: Instant,
    pub boot_completed_at: Option<Instant>,
    pub terminated_at: Option<Instant>,
    pub transitions: Vec<TransitionEvent>,
    pub peak_rss_kb: Option<u64>,
}

pub struct TransitionEvent {
    pub at: Instant,
    pub from: State,
    pub to: State,
}
