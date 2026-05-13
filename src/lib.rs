//! Ingot — minimal KVM-based VMM.
//!
//! Five subsystems, one folder each. `lifecycle` is the orchestrator
//! and owns the public API; everything else is composed by it.

// Crate-wide `dead_code` allow: `Vsock` and `VsockSpec` are sketched
// in placeholder form, and several KVM helpers (`get_regs`, `get_sregs`,
// `id`, `KVM_GET_REGS`, `KVM_GET_SREGS`, `BootParams::as_bytes`) are
// exercised by `#[cfg(test)]` blocks but not the production run loop —
// gating each call site individually with `#[cfg(test)]` would scatter
// noise across files for no diagnostic value. Real unused-import / unused-
// variable warnings still fire.
#![allow(dead_code)]

mod boot;
mod devices;
mod jailer;
mod kvm;
mod lifecycle;

pub use devices::GuestError;
pub use kvm::KvmError;
pub use lifecycle::{
    BlockSpec, Config, ExitStatus, GuestFaultReason, HostError, IngotError, NetSpec, Outcome,
    State, StdoutSink, Telemetry, TransitionEvent, Vm, VsockSpec,
};
pub use jailer::{jail_and_exec, BindMount, JailConfig, Limits, Mode as SeccompMode};
