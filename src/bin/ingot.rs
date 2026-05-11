//! Ingot CLI:
//!   ingot run --kernel PATH [--initramfs PATH] [--rootfs PATH[:ro]]
//!             [--memory MiB] [--cmdline STR] [--tap NAME]

use std::path::PathBuf;
use std::process::ExitCode;

use ingot::{BlockSpec, Config, StdoutSink, Vm};

fn parse_rootfs(value: &str) -> BlockSpec {
    let (path, readonly) = match value.strip_suffix(":ro") {
        Some(p) => (p, true),
        None => (value, false),
    };
    BlockSpec { path: PathBuf::from(path), readonly }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match cli(&args) {
        Ok(()) => ExitCode::from(0),
        Err(code) => ExitCode::from(code),
    }
}

fn cli(args: &[String]) -> Result<(), u8> {
    if args.len() < 2 || args[1] != "run" {
        eprintln!("usage: ingot run --kernel PATH [--initramfs PATH] [--memory MiB] [--cmdline STR]");
        return Err(2);
    }
    let config = parse_config(&args[2..])?;
    let outcome = Vm::new(config)
        .and_then(|vm| vm.run())
        .map_err(|e| {
            eprintln!("ingot: {:?}", e);
            1u8
        })?;
    eprintln!("ingot: exit={:?}", outcome.exit_status);
    Ok(())
}

fn parse_config(args: &[String]) -> Result<Config, u8> {
    let mut config = default_config();
    let mut i = 0;
    while i < args.len() {
        i = apply_arg(args, i, &mut config)?;
    }
    Ok(config)
}

fn apply_arg(args: &[String], i: usize, c: &mut Config) -> Result<usize, u8> {
    let name = &args[i];
    if i + 1 >= args.len() {
        eprintln!("ingot: option {} requires a value", name);
        return Err(2);
    }
    let value = &args[i + 1];
    match name.as_str() {
        "--kernel" => c.kernel_path = PathBuf::from(value),
        "--initramfs" => c.initramfs_path = Some(PathBuf::from(value)),
        "--rootfs" => c.rootfs = Some(parse_rootfs(value)),
        "--memory" => c.memory_bytes = value.parse::<u64>().unwrap_or(128) * 1024 * 1024,
        "--cmdline" => c.cmdline = value.clone(),
        "--tap" => c.tap_name = Some(value.clone()),
        other => {
            eprintln!("ingot: unknown option {}", other);
            return Err(2);
        }
    }
    Ok(i + 2)
}

fn default_config() -> Config {
    Config {
        vcpus: 1,
        memory_bytes: 128 * 1024 * 1024,
        cmdline: "console=ttyS0,115200 virtio_mmio.device=4K@0xd0000000:5 reboot=k panic=1".into(),
        stdout: StdoutSink::Fd(1),
        interactive: true,
        ..Config::default()
    }
}
