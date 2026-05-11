//! Thin wrapper around `/dev/vhost-vsock`. We hand the kernel our
//! virtqueue addresses + eventfd pairs (one kick fd + one call fd per
//! queue); after that the kernel reads guest memory directly and
//! processes the vsock protocol — we just shuttle the MMIO doorbells
//! through the kick fds and observe used-ring updates via the call fds.

use std::ffi::c_ulong;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use crate::error::HostError;

const VHOST_VIRTIO: u32 = 0xAF;

const fn ioc(dir: u32, nr: u32, size: u32) -> c_ulong {
    ((dir << 30) | (size << 16) | (VHOST_VIRTIO << 8) | nr) as c_ulong
}

const VHOST_SET_OWNER: c_ulong = ioc(0, 0x01, 0);
const VHOST_SET_FEATURES: c_ulong = ioc(1, 0x00, 8);
// SET_MEM_TABLE encodes only sizeof(struct vhost_memory) — the fixed-
// length header (nregions + padding = 8 bytes). The variable-length
// regions[] array is appended past it but isn't in the size field;
// the kernel reads nregions to know how many to grab.
const VHOST_SET_MEM_TABLE: c_ulong = ioc(1, 0x03, 8);
const VHOST_SET_VRING_NUM: c_ulong = ioc(1, 0x10, 8);
const VHOST_SET_VRING_ADDR: c_ulong = ioc(1, 0x11, 40);
const VHOST_SET_VRING_BASE: c_ulong = ioc(1, 0x12, 8);
const VHOST_SET_VRING_KICK: c_ulong = ioc(1, 0x20, 8);
const VHOST_SET_VRING_CALL: c_ulong = ioc(1, 0x21, 8);
const VHOST_VSOCK_SET_GUEST_CID: c_ulong = ioc(1, 0x60, 8);
const VHOST_VSOCK_SET_RUNNING: c_ulong = ioc(1, 0x61, 4);

#[repr(C)]
struct MemoryRegion { guest_phys: u64, memory_size: u64, userspace_addr: u64, flags_padding: u64 }
#[repr(C)]
struct MemTable { nregions: u32, padding: u32, region: MemoryRegion }
#[repr(C)]
struct VringState { index: u32, num: u32 }
#[repr(C)]
struct VringAddr { index: u32, flags: u32, desc: u64, used: u64, avail: u64, log_guest_addr: u64 }
#[repr(C)]
struct VringFile { index: u32, fd: i32 }

pub(crate) struct VhostVsock {
    fd: OwnedFd,
    /// Host userspace VA where guest physical 0 lives. Vhost's vring
    /// ADDR fields take *host* userspace addresses (no IOMMU), so we
    /// translate guest-phys → host-VA = mem_base + guest_phys before
    /// every `SET_VRING_ADDR`.
    mem_base: u64,
}

impl VhostVsock {
    pub fn open(guest_cid: u64, mem_phys: u64, mem_size: u64, host_addr: u64) -> Result<Self, HostError> {
        let raw = unsafe { libc::open(b"/dev/vhost-vsock\0".as_ptr() as _, libc::O_RDWR) };
        if raw < 0 { return Err(HostError::VhostVsockOpenFailed(errno())); }
        let v = VhostVsock { fd: unsafe { OwnedFd::from_raw_fd(raw) }, mem_base: host_addr };
        v.ioctl_w0(VHOST_SET_OWNER)?;
        v.ioctl_w(VHOST_SET_FEATURES, &0u64)?;
        let mt = MemTable { nregions: 1, padding: 0, region: MemoryRegion {
            guest_phys: mem_phys, memory_size: mem_size, userspace_addr: host_addr, flags_padding: 0,
        }};
        v.ioctl_w(VHOST_SET_MEM_TABLE, &mt)?;
        v.ioctl_w(VHOST_VSOCK_SET_GUEST_CID, &guest_cid)?;
        Ok(v)
    }

    pub fn configure_vring(&self, idx: u32, num: u32, desc: u64, avail: u64, used: u64, kick: RawFd, call: RawFd) -> Result<(), HostError> {
        let (desc, avail, used) = (desc + self.mem_base, avail + self.mem_base, used + self.mem_base);
        self.ioctl_w(VHOST_SET_VRING_NUM, &VringState { index: idx, num })?;
        self.ioctl_w(VHOST_SET_VRING_BASE, &VringState { index: idx, num: 0 })?;
        self.ioctl_w(VHOST_SET_VRING_ADDR, &VringAddr { index: idx, flags: 0, desc, used, avail, log_guest_addr: 0 })?;
        self.ioctl_w(VHOST_SET_VRING_KICK, &VringFile { index: idx, fd: kick })?;
        self.ioctl_w(VHOST_SET_VRING_CALL, &VringFile { index: idx, fd: call })?;
        Ok(())
    }

    pub fn set_running(&self, running: bool) -> Result<(), HostError> {
        self.ioctl_w(VHOST_VSOCK_SET_RUNNING, &(running as i32))
    }

    fn ioctl_w0(&self, req: c_ulong) -> Result<(), HostError> {
        let r = unsafe { libc::ioctl(self.fd.as_raw_fd(), req as _, 0) };
        if r < 0 { Err(HostError::VhostVsockIoctl(errno())) } else { Ok(()) }
    }

    fn ioctl_w<T>(&self, req: c_ulong, val: &T) -> Result<(), HostError> {
        let r = unsafe { libc::ioctl(self.fd.as_raw_fd(), req as _, val as *const _) };
        if r < 0 { Err(HostError::VhostVsockIoctl(errno())) } else { Ok(()) }
    }
}

fn errno() -> i32 { std::io::Error::last_os_error().raw_os_error().unwrap_or(0) }
