//! Ingot CLI:
//!   ingot run --kernel PATH [--initramfs PATH] [--rootfs PATH[:ro]]
//!             [--memory MiB] [--cmdline STR] [--tap NAME]
//!             [--vsock-cid N] [--timing]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use ingot::{BlockSpec, Config, Outcome, StdoutSink, Vm, VsockSpec};

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
        eprintln!("usage: ingot run --kernel PATH [--initramfs PATH] [--rootfs PATH[:ro]]");
        eprintln!("                 [--memory MiB] [--cmdline STR] [--tap NAME]");
        eprintln!("                 [--vsock-cid N] [--timing]");
        return Err(2);
    }
    let (config, timing) = parse_config(&args[2..])?;
    let t_new_start = Instant::now();
    let vm = Vm::new(config).map_err(|e| { eprintln!("ingot: {:?}", e); 1u8 })?;
    let t_new_done = Instant::now();
    let outcome = vm.run().map_err(|e| { eprintln!("ingot: {:?}", e); 1u8 })?;
    eprintln!("ingot: exit={:?}", outcome.exit_status);
    if timing { report_timing(&outcome, t_new_start, t_new_done); }
    Ok(())
}

fn report_timing(outcome: &Outcome, t_new_start: Instant, t_new_done: Instant) {
    let t = &outcome.telemetry;
    let ms = |a: Instant, b: Instant| (b.duration_since(a).as_micros() as f64) / 1000.0;
    eprintln!("--- timing (ms from process-start reference) ---");
    eprintln!("  Vm::new            {:.3}", ms(t_new_start, t_new_done));
    eprintln!("  boot-kernel        {:.3}", ms(t_new_done, t.boot_completed_at.unwrap_or(t_new_done)));
    if let Some(fo) = t.first_output_at {
        eprintln!("  first guest byte   {:.3}  (from t_new_start)", ms(t_new_start, fo));
    }
    if let Some(end) = t.terminated_at {
        eprintln!("  total              {:.3}", ms(t_new_start, end));
    }
}

fn parse_config(args: &[String]) -> Result<(Config, bool), u8> {
    let mut config = default_config();
    let mut timing = false;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--timing" { timing = true; i += 1; continue; }
        i = apply_arg(args, i, &mut config)?;
    }
    Ok((config, timing))
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
        "--vsock-cid" => c.vsock = Some(VsockSpec { guest_cid: value.parse().unwrap_or(0) }),
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
