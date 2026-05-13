use std::os::unix::io::RawFd;
use std::path::PathBuf;

use crate::lifecycle::error::HostError;

const MIN_MEMORY: u64 = 16 * 1024 * 1024;
const PAGE_SIZE: u64 = 0x1000;
const CMDLINE_MAX: usize = 2048;

#[derive(Default)]
pub struct Config {
    pub kernel_path: PathBuf,
    pub initramfs_path: Option<PathBuf>,
    pub rootfs: Option<BlockSpec>,
    pub net: Option<NetSpec>,
    pub vsock: Option<VsockSpec>,
    pub vcpus: u8,
    pub memory_bytes: u64,
    pub cmdline: String,
    pub stdout: StdoutSink,
    /// When true, lifecycle spawns a thread reading host stdin and
    /// feeds bytes into the UART's input queue. CLI sets this; library
    /// callers leave it false.
    pub interactive: bool,
    /// Optional host-side TAP interface name to bind virtio-net to.
    /// Caller is responsible for pre-creating the device with the
    /// appropriate ownership (`ip tuntap add <name> mode tap user X`).
    pub tap_name: Option<String>,
}

impl Config {
    /// Reject configurations that cannot produce a runnable Vm. Called
    /// at the top of `Vm::new` before any KVM resource is touched.
    pub fn validate(&self) -> Result<(), HostError> {
        if !self.kernel_path.is_file() {
            return Err(HostError::KernelImageNotReadable(self.kernel_path.clone()));
        }
        if let Some(p) = &self.initramfs_path {
            if !p.is_file() {
                return Err(HostError::InitramfsNotReadable(p.clone()));
            }
        }
        if self.vcpus != 1 {
            return Err(HostError::InvalidConfig("only single-vCPU is supported"));
        }
        if self.memory_bytes < MIN_MEMORY {
            return Err(HostError::InvalidConfig("memory must be at least 16 MiB"));
        }
        if !self.memory_bytes.is_multiple_of(PAGE_SIZE) {
            return Err(HostError::InvalidConfig("memory must be page-aligned"));
        }
        if self.cmdline.len() > CMDLINE_MAX {
            return Err(HostError::InvalidConfig("cmdline too long (max 2048 bytes)"));
        }
        if self.cmdline.contains('\0') {
            return Err(HostError::InvalidConfig("cmdline contains NUL"));
        }
        Ok(())
    }
}

pub struct BlockSpec {
    pub path: PathBuf,
    pub readonly: bool,
}

pub struct NetSpec {
    pub tap_fd: RawFd,
}

pub struct VsockSpec {
    /// Guest's vsock Context ID. The host's CID is fixed at 2
    /// (`VMADDR_CID_HOST`); pick anything >= 3 for the guest, unique
    /// across concurrent VMs on this host. With vhost-vsock backing,
    /// any host process can `socket(AF_VSOCK)` + `connect((cid, port))`
    /// to reach the guest — no fd needs to flow through ingot.
    pub guest_cid: u32,
}

#[derive(Default)]
pub enum StdoutSink {
    #[default]
    Discard,
    Buffer,
    File(PathBuf),
    Fd(RawFd),
}
