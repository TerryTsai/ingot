//! Typed-harness spike (Phase 0). See docs/spikes/typed-harness.md.
//!
//! Compiles against the Phase 0 API. Does not run.

#![allow(dead_code, unused_imports, unused_variables)]

mod cmd;
mod eval;
mod task;

use cmd::CommandTask;
use eval::{EvalTask, Language};
use task::{Host, Task};

fn main() {
    let host = Host::new(
        "/var/lib/ingot/vmlinux",
        "/var/lib/ingot/eval-rootfs.cpio",
    );

    let eval = EvalTask {
        source: "print(1 + 2)".into(),
        language: Language::Python,
    };
    println!("eval plan: {}", eval.plan().summary);
    if let Ok(run) = eval.execute(&host) {
        println!(
            "eval -> {} ({} ms, {} bytes out)",
            run.output.stdout, run.manifest.duration_ms, run.manifest.bytes_out
        );
    }

    let cmd = CommandTask {
        program: "/usr/bin/wc".into(),
        args: vec!["-l".into()],
        stdin: b"hello\nworld\n".to_vec(),
    };
    println!("cmd plan: {}", cmd.plan().summary);
    if let Ok(run) = cmd.execute(&host) {
        println!(
            "cmd -> exit {}, {} stdout bytes",
            run.output.exit_code,
            run.output.stdout.len()
        );
    }
}
