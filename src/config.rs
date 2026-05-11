use std::os::unix::io::RawFd;
use std::path::PathBuf;

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

pub struct BlockSpec {
    pub path: PathBuf,
    pub readonly: bool,
}

pub struct NetSpec {
    pub tap_fd: RawFd,
}

pub struct VsockSpec {
    pub host_listener_fd: RawFd,
    pub guest_cid: u32,
}

pub enum StdoutSink {
    Discard,
    Buffer,
    File(PathBuf),
    Fd(RawFd),
}

impl Default for StdoutSink {
    fn default() -> Self {
        StdoutSink::Discard
    }
}
