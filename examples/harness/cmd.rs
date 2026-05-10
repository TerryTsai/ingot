//! Sample task: run a program with stdin; capture stdout, stderr, exit code.

use std::time::Duration;

use ingot::IngotError;

use crate::task::{Host, Plan, Run, Task};

pub struct CommandTask {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
}

pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

pub enum CommandError {
    Vmm(IngotError),
    GuestProtocol(&'static str),
}

impl From<IngotError> for CommandError {
    fn from(e: IngotError) -> Self {
        CommandError::Vmm(e)
    }
}

impl Task for CommandTask {
    type Output = CommandOutput;
    type Error = CommandError;

    fn plan(&self) -> Plan {
        Plan {
            kind: "command",
            summary: format!(
                "{} with {} arg(s), {} stdin bytes",
                self.program,
                self.args.len(),
                self.stdin.len()
            ),
            estimated_duration: Duration::from_secs(5),
        }
    }

    fn execute(&self, host: &Host) -> Result<Run<CommandOutput>, CommandError> {
        // Same skeleton as EvalTask: vsock listener → Config → worker
        // thread → wire protocol → Manifest. Different wire payload
        // (program/args/stdin in; stdout/stderr/exit_code out).
        unimplemented!()
    }
}
