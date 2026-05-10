//! Ingot — minimal KVM-based VMM. See SPEC.md and docs/architecture.md.
//!
//! Phase 0 sketch: types and signatures only. Method bodies panic via
//! `unimplemented!()`. Crate-level allows are sketch-scoped and removed
//! as subsystems are implemented.

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

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
