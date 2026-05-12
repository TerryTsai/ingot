//! Internal mechanics for `Vm` — guest memory, host-I/O wiring for
//! the UART, and the SIGALRM-driven KVM_RUN wakeup. Not part of the
//! public surface; not part of the orchestrator's narrative. Nested
//! so the eye lands on the primary surface (vm.rs + config + outcome
//! + error + state) first.

pub(crate) mod memory;
pub(crate) mod stdio;
pub(crate) mod wakeup_timer;
