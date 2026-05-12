//! ingot-jailer: out-of-process isolation layer for the ingot VMM.
//!
//! Runs as root, sets up a cgroup + namespaces + a minimal rootfs +
//! drops privilege, then `execve`s the ingot binary. After exec the
//! VMM inherits all four layers; combined with its in-process seccomp
//! filter, it has no way to read host files outside declared mounts,
//! open network connections outside its declared interface, exceed
//! resource limits, or make out-of-allowlist syscalls.
//!
//! Usage:
//!   ingot-jailer \
//!     --cgroup NAME --memory-max BYTES --pids-max N \
//!     --uid UID --gid GID \
//!     --root /path/to/empty/dir \
//!     --bind SRC[:DST]  (repeat)... \
//!     --ingot /path/to/ingot \
//!     -- [args forwarded to ingot]

use std::path::PathBuf;
use std::process::ExitCode;

use ingot::{jail_and_exec, BindMount, JailConfig, Limits};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(config) => match jail_and_exec(config) {
            Ok(code) => code,
            Err(e) => { eprintln!("ingot-jailer: {e}"); ExitCode::from(1) }
        },
        Err(e) => { eprintln!("ingot-jailer: {e}"); ExitCode::from(2) }
    }
}

fn parse_args(args: &[String]) -> Result<JailConfig, String> {
    let mut cgroup_name = String::new();
    let mut memory_max: Option<u64> = None;
    let mut pids_max: Option<u32> = None;
    let mut cpu_weight: Option<u32> = None;
    let mut uid: Option<u32> = None;
    let mut gid: Option<u32> = None;
    let mut supp_groups: Vec<u32> = Vec::new();
    let mut new_root: Option<PathBuf> = None;
    let mut binds: Vec<BindMount> = Vec::new();
    let mut ingot_path: Option<PathBuf> = None;
    let mut forwarded_args: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" { forwarded_args.extend(args[i + 1..].iter().cloned()); break; }
        let next = || args.get(i + 1).cloned().ok_or_else(|| format!("missing value for {a}"));
        match a.as_str() {
            "--cgroup"      => { cgroup_name = next()?; i += 2; }
            "--memory-max"  => { memory_max = Some(parse_size(&next()?)?); i += 2; }
            "--pids-max"    => { pids_max = Some(next()?.parse().map_err(|_| "bad --pids-max")?); i += 2; }
            "--cpu-weight"  => { cpu_weight = Some(next()?.parse().map_err(|_| "bad --cpu-weight")?); i += 2; }
            "--uid"         => { uid = Some(next()?.parse().map_err(|_| "bad --uid")?); i += 2; }
            "--gid"         => { gid = Some(next()?.parse().map_err(|_| "bad --gid")?); i += 2; }
            "--supp-groups" => {
                supp_groups = next()?.split(',')
                    .map(|s| s.parse::<u32>().map_err(|_| format!("bad --supp-groups gid: {s}")))
                    .collect::<Result<Vec<_>, _>>()?;
                i += 2;
            }
            "--root"        => { new_root = Some(PathBuf::from(next()?)); i += 2; }
            "--bind"        => { binds.push(parse_bind(&next()?)?); i += 2; }
            "--ingot"       => { ingot_path = Some(PathBuf::from(next()?)); i += 2; }
            other           => return Err(format!("unknown flag {other}")),
        }
    }

    Ok(JailConfig {
        cgroup_name:     require(cgroup_name, "--cgroup")?,
        limits:          Limits { memory_max, pids_max, cpu_weight },
        uid:             uid.ok_or("--uid required")?,
        gid:             gid.ok_or("--gid required")?,
        supp_groups,
        new_root:        new_root.ok_or("--root required")?,
        binds,
        ingot_path:      ingot_path.ok_or("--ingot required")?,
        forwarded_args,
    })
}

fn require(s: String, name: &str) -> Result<String, String> {
    if s.is_empty() { Err(format!("{name} required")) } else { Ok(s) }
}

/// "SRC" or "SRC:DST" → BindMount.
fn parse_bind(spec: &str) -> Result<BindMount, String> {
    let (src, dst) = match spec.split_once(':') {
        Some((s, d)) => (s, d),
        None => (spec, spec),
    };
    Ok(BindMount { source: PathBuf::from(src), target: PathBuf::from(dst) })
}

/// Accept "256M", "1G", "12345" (bytes).
fn parse_size(s: &str) -> Result<u64, String> {
    let (digits, mult): (&str, u64) = match s.chars().last() {
        Some('K') | Some('k') => (&s[..s.len() - 1], 1024),
        Some('M') | Some('m') => (&s[..s.len() - 1], 1024 * 1024),
        Some('G') | Some('g') => (&s[..s.len() - 1], 1024 * 1024 * 1024),
        _ => (s, 1),
    };
    digits.parse::<u64>()
        .map(|n| n * mult)
        .map_err(|_| format!("bad size {s}"))
}
