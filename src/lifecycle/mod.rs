//! VM lifecycle: validate config, instantiate subsystems, drive the
//! run-to-completion loop, capture telemetry. The only subsystem that
//! knows about all the others.

mod run;
mod runtime;
mod state;
mod validate;

#[cfg(test)]
mod tests;

pub use state::State;
pub(crate) use runtime::Runtime;

use crate::error::IngotError;
use crate::outcome::Outcome;

pub(crate) fn run(runtime: Runtime) -> Result<Outcome, IngotError> {
    run::run(runtime)
}
