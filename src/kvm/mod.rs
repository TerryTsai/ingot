//! KVM ioctl wrappers. The three Fd primitives (`KvmFd`, `VmFd`,
//! `VcpuFd`) own their respective KVM file descriptors and expose the
//! minimum surface the orchestrator needs.

use std::io;

use kvm_bindings::{kvm_regs, kvm_sregs};

mod cpuid;
mod ioctls;
mod kvm_fd;
mod vcpu_exit;
mod vcpu_fd;
mod vm_fd;

#[cfg(test)]
pub(crate) mod test_helpers;

pub(crate) use cpuid::Cpuid;
pub(crate) use kvm_fd::KvmFd;
pub use vcpu_exit::VcpuExit;
pub(crate) use vcpu_fd::VcpuFd;
pub(crate) use vm_fd::VmFd;

pub struct RegState {
    pub regs: kvm_regs,
    pub sregs: kvm_sregs,
}

impl RegState {
    pub fn zeroed() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[derive(Debug)]
pub enum KvmError {
    Open(i32),
    AbiVersionMismatch { got: i32, want: u32 },
    Ioctl { name: &'static str, errno: i32 },
    Mmap(i32),
    UnhandledExit(u32),
}

pub(crate) fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

pub(crate) fn check(r: libc::c_int, name: &'static str) -> Result<libc::c_int, KvmError> {
    if r < 0 {
        Err(KvmError::Ioctl { name, errno: errno() })
    } else {
        Ok(r)
    }
}
