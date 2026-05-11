//! Jailer subsystem (SPEC §3.1.5). Currently the in-process half: a
//! seccomp BPF allowlist the VMM installs on itself before entering
//! the run loop. The out-of-process half — a separate `ingot-jailer`
//! binary that does cgroups + namespaces + privilege drop and execve's
//! the VMM — is the remaining Phase 7 work.

mod seccomp;

pub use seccomp::{install, Mode};
