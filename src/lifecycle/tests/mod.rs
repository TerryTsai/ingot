//! Lifecycle integration tests. Requires /dev/kvm for the end-to-end suite.

mod end_to_end;
mod state;
mod validate;

use crate::{Config, IngotError, Outcome, StdoutSink, Vm};

pub(super) const ENTRY_POINT: u64 = 0x10_0000;

pub(super) fn run_with_payload(name: &str, payload: &[u8]) -> Result<Outcome, IngotError> {
    run_with_stdout(name, payload, StdoutSink::Discard)
}

pub(super) fn run_with_stdout(
    name: &str,
    payload: &[u8],
    stdout: StdoutSink,
) -> Result<Outcome, IngotError> {
    let mut p = std::env::temp_dir();
    p.push(format!("ingot-synth-{}-{}.elf", name, std::process::id()));
    std::fs::write(&p, build_elf(payload)).expect("write tmp");
    let cfg = Config {
        kernel_path: p.clone(),
        vcpus: 1,
        memory_bytes: 16 * 1024 * 1024,
        cmdline: String::new(),
        stdout,
        ..Config::default()
    };
    let result = Vm::new(cfg).and_then(|vm| vm.run());
    let _ = std::fs::remove_file(&p);
    result
}

fn build_elf(payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(64 + 56 + payload.len());
    b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&62u16.to_le_bytes());
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
    b.extend_from_slice(&64u64.to_le_bytes());
    b.extend_from_slice(&0u64.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes());
    b.extend_from_slice(&64u16.to_le_bytes());
    b.extend_from_slice(&56u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&[0u8; 6]);
    b.extend_from_slice(&1u32.to_le_bytes());
    b.extend_from_slice(&5u32.to_le_bytes());
    b.extend_from_slice(&(64u64 + 56).to_le_bytes());
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
    let psz = payload.len() as u64;
    b.extend_from_slice(&psz.to_le_bytes());
    b.extend_from_slice(&psz.to_le_bytes());
    b.extend_from_slice(&0x1000u64.to_le_bytes());
    b.extend_from_slice(payload);
    b
}
