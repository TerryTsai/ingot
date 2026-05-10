//! Ingot — minimal KVM-based VMM. See SPEC.md and docs/architecture.md.

// `dead_code` stays crate-wide while virtio devices and IRQ injection remain
// placeholders. Real-bug allows (unused imports/variables) are not granted.
#![allow(dead_code)]

mod config;
mod error;
mod guest_mem;
mod outcome;

mod boot;
mod devices;
mod kvm;
mod lifecycle;

pub use config::{BlockSpec, Config, NetSpec, StdoutSink, VsockSpec};
pub use devices::GuestError;
pub use error::{HostError, IngotError};
pub use kvm::KvmError;
pub use lifecycle::State;
pub use outcome::{ExitStatus, GuestFaultReason, Outcome, Telemetry, TransitionEvent};

pub struct Vm {
    runtime: lifecycle::Runtime,
}

impl Vm {
    pub fn new(config: Config) -> Result<Self, IngotError> {
        Ok(Vm {
            runtime: lifecycle::Runtime::new(config)?,
        })
    }

    pub fn run(self) -> Result<Outcome, IngotError> {
        lifecycle::run(self.runtime)
    }
}
