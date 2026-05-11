//! Ingot — minimal KVM-based VMM. See SPEC.md and docs/architecture.md.

// `dead_code` stays crate-wide while virtio devices and IRQ injection remain
// placeholders. Real-bug allows (unused imports/variables) are not granted.
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
