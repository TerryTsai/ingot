//! VM lifecycle state machine and run-to-completion loop.
//!
//! Phase 6 deliverable. Currently a sketch.

mod state;

pub use state::State;

use crate::config::Config;
use crate::devices::MmioBus;
use crate::error::IngotError;
use crate::guest_mem::GuestMem;
use crate::kvm::{VcpuFd, VmFd};
use crate::outcome::{Outcome, Telemetry};

pub(crate) struct Runtime {
    state: State,
    config: Config,
    mem: GuestMem,
    vm_fd: VmFd,
    vcpus: Vec<VcpuFd>,
    bus: MmioBus,
    telemetry: Telemetry,
}

impl Runtime {
    pub fn new(config: Config) -> Result<Self, IngotError> {
        unimplemented!()
    }
}

pub(crate) fn run(runtime: Runtime) -> Result<Outcome, IngotError> {
    unimplemented!()
}
