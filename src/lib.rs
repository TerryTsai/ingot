//! Ingot — minimal KVM-based VMM. See SPEC.md and docs/architecture.md.

// Crate-wide `dead_code` allow: Phase 5 (`Vsock`, `VsockSpec`) is sketched
// in placeholder form, and several KVM helpers (`get_regs`, `get_sregs`,
// `id`, `KVM_GET_REGS`, `KVM_GET_SREGS`, `BootParams::as_bytes`) are
// exercised by `#[cfg(test)]` blocks but not the production run loop —
// gating each call site individually with `#[cfg(test)]` would scatter
// noise across files for no diagnostic value. Real unused-import / unused-
// variable warnings still fire.
#![allow(dead_code)]

mod config;
mod error;
mod memory;
mod outcome;
mod state;

mod boot;
mod devices;
mod kvm;
mod vm;

pub use config::{BlockSpec, Config, NetSpec, StdoutSink, VsockSpec};
pub use devices::GuestError;
pub use error::{HostError, IngotError};
pub use kvm::KvmError;
pub use outcome::{ExitStatus, GuestFaultReason, Outcome, Telemetry, TransitionEvent};
pub use state::State;
pub use vm::Vm;
