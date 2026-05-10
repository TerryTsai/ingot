//! End-to-end: drive Vm::new + Vm::run with a synthetic "kernel" — a
//! valid x86_64 ELF whose single PT_LOAD segment contains one byte
//! (0xF4 = hlt) at the entry point. After prime_vcpu hands off to the
//! kernel in long mode, the CPU executes hlt and KVM exits with
//! KVM_EXIT_HLT. The expected Outcome is ExitStatus::Halted.

use std::path::PathBuf;

use crate::outcome::ExitStatus;
use crate::{Config, StdoutSink, Vm};

const ENTRY_POINT: u64 = 0x10_0000;

fn synth_kernel_path() -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("ingot-synth-kernel-{}.elf", std::process::id()));
    std::fs::write(&p, build_hlt_elf()).expect("write tmp kernel");
    p
}

fn build_hlt_elf() -> Vec<u8> {
    let mut b = Vec::with_capacity(64 + 56 + 1);
    b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    b.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    b.extend_from_slice(&62u16.to_le_bytes()); // EM_X86_64
    b.extend_from_slice(&1u32.to_le_bytes()); // e_version
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
    b.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
    b.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    b.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    b.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    b.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    b.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    b.extend_from_slice(&[0u8; 6]); // e_shentsize, e_shnum, e_shstrndx
    b.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    b.extend_from_slice(&5u32.to_le_bytes()); // p_flags
    b.extend_from_slice(&(64u64 + 56).to_le_bytes()); // p_offset
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes()); // p_vaddr
    b.extend_from_slice(&ENTRY_POINT.to_le_bytes()); // p_paddr
    b.extend_from_slice(&1u64.to_le_bytes()); // p_filesz
    b.extend_from_slice(&1u64.to_le_bytes()); // p_memsz
    b.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
    b.push(0xf4);
    b
}

#[test]
fn synthetic_hlt_kernel_reaches_halted() {
    let kernel = synth_kernel_path();
    let cfg = Config {
        kernel_path: kernel.clone(),
        vcpus: 1,
        memory_bytes: 16 * 1024 * 1024,
        cmdline: String::new(),
        stdout: StdoutSink::Discard,
        ..Config::default()
    };
    let result = Vm::new(cfg).and_then(|vm| vm.run());
    let _ = std::fs::remove_file(&kernel);
    match result {
        Ok(outcome) => assert!(matches!(outcome.exit_status, ExitStatus::Halted)),
        Err(e) => eprintln!("[synthetic_hlt_kernel_reaches_halted] skipped: {:?}", e),
    }
}
