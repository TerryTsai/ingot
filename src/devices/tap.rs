//! Host-side TAP device. Binds to a pre-existing `tap<N>` interface
//! created by the host operator (`ip tuntap add tapN mode tap user
//! <vmm-user>`). Once bound, the fd is the L2 frame transport: writes
//! deliver Ethernet frames into the host's network stack; reads receive
//! frames sent toward this interface from the host side.

use std::ffi::{c_int, c_short};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use crate::lifecycle::error::HostError;

const TUNSETIFF: u64 = 0x4004_54ca;
const IFF_TAP: c_short = 0x0002;
const IFF_NO_PI: c_short = 0x1000;
const IFNAMSIZ: usize = 16;
const DEV_NET_TUN: &[u8] = b"/dev/net/tun\0";

#[repr(C)]
struct Ifreq {
    name: [u8; IFNAMSIZ],
    flags: c_short,
    _pad: [u8; 22],
}

pub(crate) struct Tap {
    fd: OwnedFd,
}

impl Tap {
    pub fn open(name: &str) -> Result<Self, HostError> {
        let raw = unsafe { libc::open(DEV_NET_TUN.as_ptr() as _, libc::O_RDWR) };
        if raw < 0 { return Err(HostError::TapOpenFailed(errno())); }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut req = Ifreq { name: [0; IFNAMSIZ], flags: IFF_TAP | IFF_NO_PI, _pad: [0; 22] };
        let bytes = name.as_bytes();
        if bytes.len() >= IFNAMSIZ { return Err(HostError::InvalidConfig("tap name too long")); }
        req.name[..bytes.len()].copy_from_slice(bytes);
        let r = unsafe { libc::ioctl(fd.as_raw_fd(), TUNSETIFF as _, &mut req) };
        if r < 0 { return Err(HostError::TapOpenFailed(errno())); }
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
        unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
        Ok(Tap { fd })
    }

    pub fn write(&self, frame: &[u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::write(self.fd.as_raw_fd(), frame.as_ptr() as _, frame.len()) };
        if n < 0 { Err(std::io::Error::last_os_error()) } else { Ok(n as usize) }
    }

    /// Returns `Ok(0)` when no frame is currently available (EAGAIN).
    pub fn read(&self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = unsafe { libc::read(self.fd.as_raw_fd(), buf.as_mut_ptr() as _, buf.len()) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EAGAIN) { Ok(0) } else { Err(err) }
        } else {
            Ok(n as usize)
        }
    }

}

fn errno() -> c_int { std::io::Error::last_os_error().raw_os_error().unwrap_or(0) }
