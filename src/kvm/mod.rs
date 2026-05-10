//! KVM ioctl wrapper.
//!
//! Phase 1: opens /dev/kvm, creates VMs and vCPUs, registers guest memory,
//! runs vCPUs, surfaces exit reasons. No knowledge of Linux boot, virtio,
//! lifecycle, or configuration — those are other subsystems' concerns.

use std::io;

use kvm_bindings::{kvm_regs, kvm_sregs};

mod exit;
mod host;
mod ioctls;
mod vcpu;
mod vm;

#[cfg(test)]
mod tests;

pub use exit::VcpuExit;
pub use host::Kvm;
pub use vcpu::VcpuFd;
pub use vm::VmFd;

const DEV_KVM: &[u8] = b"/dev/kvm\0";

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
