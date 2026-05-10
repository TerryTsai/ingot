use std::path::PathBuf;

use crate::{Config, StdoutSink, Vm};

fn touch(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ingot-validate-{}-{}", name, std::process::id()));
    std::fs::write(&p, b"placeholder").unwrap();
    p
}

fn base() -> Config {
    Config {
        vcpus: 1,
        memory_bytes: 16 * 1024 * 1024,
        stdout: StdoutSink::Discard,
        ..Config::default()
    }
}

#[test]
fn rejects_missing_kernel() {
    let mut cfg = base();
    cfg.kernel_path = PathBuf::from("/nonexistent/kernel/path");
    let r = Vm::new(cfg);
    assert!(r.is_err());
}

#[test]
fn rejects_zero_vcpus() {
    let mut cfg = base();
    cfg.kernel_path = touch("zero_vcpus");
    cfg.vcpus = 0;
    let r = Vm::new(cfg);
    let _ = std::fs::remove_file("/tmp/x"); // best-effort
    assert!(r.is_err());
}

#[test]
fn rejects_too_small_memory() {
    let mut cfg = base();
    cfg.kernel_path = touch("small_mem");
    cfg.memory_bytes = 1024;
    assert!(Vm::new(cfg).is_err());
}

#[test]
fn rejects_unpaged_memory() {
    let mut cfg = base();
    cfg.kernel_path = touch("unpaged_mem");
    cfg.memory_bytes = 16 * 1024 * 1024 + 7;
    assert!(Vm::new(cfg).is_err());
}

#[test]
fn rejects_cmdline_with_nul() {
    let mut cfg = base();
    cfg.kernel_path = touch("nul_cmdline");
    cfg.cmdline = "a\0b".into();
    assert!(Vm::new(cfg).is_err());
}
