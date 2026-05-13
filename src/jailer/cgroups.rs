//! cgroups v2 setup. Creates a fresh cgroup under `/sys/fs/cgroup`,
//! writes the configured limits (memory.max, pids.max, cpu.weight),
//! and moves the current process into it. After this returns the
//! process and any children inherit the limits.
//!
//! Requires the unified cgroups v2 hierarchy mounted at /sys/fs/cgroup,
//! which is the Linux default since systemd 246 / kernel 4.5. v1 is
//! not supported.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";

pub struct Cgroup {
    path: PathBuf,
}

pub struct Limits {
    /// `memory.max` in bytes, or None for no cap.
    pub memory_max: Option<u64>,
    /// `pids.max` (max process count), or None for no cap.
    pub pids_max: Option<u32>,
    /// `cpu.weight` (1..=10_000, default 100), or None for default.
    pub cpu_weight: Option<u32>,
}

impl Cgroup {
    /// Create a new cgroup `<CGROUP_ROOT>/<name>`, apply `limits`, move
    /// the current process into it. Idempotent on the dir (won't fail
    /// if it already exists from a prior aborted run).
    pub fn create(name: &str, limits: &Limits) -> io::Result<Self> {
        let path = PathBuf::from(CGROUP_ROOT).join(name);
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
        if let Some(m) = limits.memory_max {
            fs::write(path.join("memory.max"), m.to_string())?;
        }
        if let Some(p) = limits.pids_max {
            fs::write(path.join("pids.max"), p.to_string())?;
        }
        if let Some(w) = limits.cpu_weight {
            fs::write(path.join("cpu.weight"), w.to_string())?;
        }
        Self::add_pid(&path, std::process::id())?;
        Ok(Cgroup { path })
    }

    pub fn path(&self) -> &Path { &self.path }

    fn add_pid(cgroup_path: &Path, pid: u32) -> io::Result<()> {
        fs::write(cgroup_path.join("cgroup.procs"), pid.to_string())
    }
}
