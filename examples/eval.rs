//! Worked library example. Boots a microVM, runs whatever workload is
//! in the initramfs, captures stdout, returns it as a `String`.
//!
//! Run with:
//!   cargo run --example eval -- vmlinux initramfs.cpio.gz
//!
//! Sketch for the "code-runner" shape sketched in
//! `docs/spikes/embedded-api-ergonomics.md`. The full vision feeds
//! code to a generic interpreter image via vsock (Phase 5); this
//! version is the pre-vsock approximation — pack code into the
//! initramfs and read back the stdout the workload prints.

use std::path::PathBuf;

use ingot::{Config, IngotError, StdoutSink, Vm};

fn evaluate(kernel: PathBuf, initramfs: PathBuf) -> Result<String, IngotError> {
    let cfg = Config {
        kernel_path: kernel,
        initramfs_path: Some(initramfs),
        vcpus: 1,
        memory_bytes: 128 * 1024 * 1024,
        cmdline: "console=ttyS0,115200 reboot=k panic=1".into(),
        stdout: StdoutSink::Buffer,
        ..Config::default()
    };
    let outcome = Vm::new(cfg)?.run()?;
    Ok(String::from_utf8_lossy(&outcome.stdout).into_owned())
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: eval <kernel> <initramfs>");
        std::process::exit(2);
    }
    match evaluate(PathBuf::from(&args[1]), PathBuf::from(&args[2])) {
        Ok(out) => {
            println!("--- guest stdout ({} bytes) ---", out.len());
            print!("{out}");
        }
        Err(e) => {
            eprintln!("eval failed: {e:?}");
            std::process::exit(1);
        }
    }
}
