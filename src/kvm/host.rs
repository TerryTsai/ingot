//! Kvm: handle to /dev/kvm. Owns the host's KVM device fd.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use kvm_bindings::KVM_API_VERSION;

use super::ioctls::*;
use super::{check, errno, KvmError, VmFd, DEV_KVM};

pub struct Kvm {
    fd: OwnedFd,
    pub(super) vcpu_mmap_size: usize,
}

impl Kvm {
    pub fn open() -> Result<Self, KvmError> {
        let raw = unsafe {
            libc::open(DEV_KVM.as_ptr() as _, libc::O_RDWR | libc::O_CLOEXEC)
        };
        if raw < 0 {
            return Err(KvmError::Open(errno()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let api = check(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_API_VERSION, 0) },
            "KVM_GET_API_VERSION",
        )?;
        if api as u32 != KVM_API_VERSION {
            return Err(KvmError::AbiVersionMismatch {
                got: api,
                want: KVM_API_VERSION,
            });
        }
        let mmap = check(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_VCPU_MMAP_SIZE, 0) },
            "KVM_GET_VCPU_MMAP_SIZE",
        )?;
        Ok(Kvm {
            fd,
            vcpu_mmap_size: mmap as usize,
        })
    }

    pub fn create_vm(&self) -> Result<VmFd, KvmError> {
        let raw = check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_VM, 0) },
            "KVM_CREATE_VM",
        )?;
        Ok(VmFd::new(unsafe { OwnedFd::from_raw_fd(raw) }, self.vcpu_mmap_size))
    }
}
