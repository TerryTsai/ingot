//! Embedded API ergonomics spike — call-site for a hypothetical code-runner.
//!
//! Compiles against the Phase 0 API sketch but does not run; every library
//! function body is `unimplemented!()`.
//!
//! This is the *simple* shape — three lines of meaningful API. It does
//! not actually deliver `code` to the guest; the API offers no good path
//! for that. The realistic shape is materially larger; see
//! docs/spikes/embedded-api-ergonomics.md.

use std::path::PathBuf;

use ingot::{Config, IngotError, StdoutSink, Vm};

fn evaluate(_code: &str) -> Result<String, IngotError> {
    let cfg = Config {
        kernel_path: PathBuf::from("/var/lib/ingot/vmlinux"),
        initramfs_path: Some(PathBuf::from("/var/lib/ingot/eval-rootfs.cpio")),
        vcpus: 1,
        memory_bytes: 128 * 1024 * 1024,
        cmdline: "console=ttyS0 reboot=k panic=1 quiet".into(),
        stdout: StdoutSink::Buffer,
        ..Config::default()
    };

    let outcome = Vm::new(cfg)?.run()?;
    Ok(String::from_utf8_lossy(&outcome.stdout).into_owned())
}

fn main() {
    match evaluate("print(1 + 2)") {
        Ok(out) => println!("guest stdout: {out}"),
        Err(_) => std::process::exit(1),
    }
}
