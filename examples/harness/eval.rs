//! Sample task: evaluate a snippet of source code.

use std::time::Duration;

use ingot::IngotError;

use crate::task::{Host, Plan, Run, Task};

#[derive(Debug, Clone, Copy)]
pub enum Language {
    Python,
    JavaScript,
    Bash,
}

pub struct EvalTask {
    pub source: String,
    pub language: Language,
}

pub struct EvalOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub enum EvalError {
    Vmm(IngotError),
    InvalidSource(&'static str),
    GuestProtocol(&'static str),
}

impl From<IngotError> for EvalError {
    fn from(e: IngotError) -> Self {
        EvalError::Vmm(e)
    }
}

impl Task for EvalTask {
    type Output = EvalOutput;
    type Error = EvalError;

    fn plan(&self) -> Plan {
        Plan {
            kind: "eval",
            summary: format!(
                "eval {} bytes of {:?}",
                self.source.len(),
                self.language
            ),
            estimated_duration: Duration::from_secs(2),
        }
    }

    fn execute(&self, host: &Host) -> Result<Run<EvalOutput>, EvalError> {
        // Sketch only. The realistic shape is:
        // 1. open AF_VSOCK listener; today this requires libc-level Rust
        //    (ergonomics-spike friction #3).
        // 2. host.base_config(host_fd, cid) → Config.
        // 3. spawn ingot::Vm::new(cfg)?.run() on a worker thread because
        //    Vm::run blocks (ergonomics-spike friction #2).
        // 4. accept the guest connection, send (language, source), read
        //    (stdout, stderr, exit_code) per a small wire protocol the
        //    initramfs's init implements.
        // 5. join worker thread; build Manifest from byte counts +
        //    Outcome.exit_status + Telemetry.
        unimplemented!()
    }
}
