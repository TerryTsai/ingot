//! Harness primitives: Task trait, Plan, Run, Manifest, Host.

use std::path::PathBuf;
use std::time::Duration;

use ingot::{Config, IngotError};

pub struct Host {
    pub kernel_path: PathBuf,
    pub initramfs_path: PathBuf,
    pub vcpus: u8,
    pub memory_bytes: u64,
}

impl Host {
    pub fn new<P: Into<PathBuf>>(kernel: P, initramfs: P) -> Self {
        Host {
            kernel_path: kernel.into(),
            initramfs_path: initramfs.into(),
            vcpus: 1,
            memory_bytes: 128 * 1024 * 1024,
        }
    }

    pub fn base_config(&self, vsock_fd: i32, guest_cid: u32) -> Config {
        unimplemented!("would assemble the Config common to every task")
    }
}

pub trait Task {
    type Output;
    type Error: From<IngotError>;

    fn plan(&self) -> Plan;
    fn execute(&self, host: &Host) -> Result<Run<Self::Output>, Self::Error>;
}

pub struct Plan {
    pub kind: &'static str,
    pub summary: String,
    pub estimated_duration: Duration,
}

pub struct Run<T> {
    pub output: T,
    pub manifest: Manifest,
}

pub struct Manifest {
    pub task_kind: &'static str,
    pub duration_ms: u128,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub vm_exit: ingot::ExitStatus,
}
