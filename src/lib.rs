//! Ingot — minimal KVM-based VMM. See SPEC.md and docs/architecture.md.
//!
//! The five subsystems from SPEC §3.1 each live in a folder of the
//! same name. `lifecycle` is the orchestrator and the public-API
//! owner; everything else is composed by it.

// Crate-wide `dead_code` allow: Phase 5 (`Vsock`, `VsockSpec`) is sketched
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
pub use jailer::{install as install_seccomp, Mode as SeccompMode};
