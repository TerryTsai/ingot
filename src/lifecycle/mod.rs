//! Lifecycle subsystem. Owns the `Vm` orchestrator, the
//! state machine, the public configuration + outcome surface, the
//! single mmap'd guest-memory region, and the unified `IngotError`
//! dispatch. Composes `kvm`, `boot`, and `devices`; is the only
//! subsystem that knows about all of them.

mod config;
pub(crate) mod error;
mod outcome;
pub(crate) mod state;
pub(crate) mod support;
mod vm;

pub use config::{BlockSpec, Config, NetSpec, StdoutSink, VsockSpec};
pub use error::{HostError, IngotError};
pub use outcome::{ExitStatus, GuestFaultReason, Outcome, Telemetry, TransitionEvent};
pub use state::State;
pub use vm::Vm;
