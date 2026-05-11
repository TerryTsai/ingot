//! Handle to /dev/kvm. Opens the device, validates the ABI version,
//! and creates VMs.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use kvm_bindings::KVM_API_VERSION;

use super::ioctls::*;
use super::vm_fd::VmFd;
use super::{check, errno, KvmError};

const DEV_KVM: &[u8] = b"/dev/kvm\0";

pub(crate) struct KvmFd {
    fd: OwnedFd,
    pub(super) vcpu_mmap_size: usize,
}

impl KvmFd {
    pub fn open() -> Result<Self, KvmError> {
        let raw = unsafe { libc::open(DEV_KVM.as_ptr() as _, libc::O_RDWR | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(KvmError::Open(errno()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let api = check(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_API_VERSION, 0) },
            "KVM_GET_API_VERSION",
        )?;
        if api as u32 != KVM_API_VERSION {
            return Err(KvmError::AbiVersionMismatch { got: api, want: KVM_API_VERSION });
        }
        let mmap = check(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_VCPU_MMAP_SIZE, 0) },
            "KVM_GET_VCPU_MMAP_SIZE",
        )?;
        Ok(KvmFd { fd, vcpu_mmap_size: mmap as usize })
    }

    pub fn create_vm(&self) -> Result<VmFd, KvmError> {
        let raw = check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_VM, 0) },
            "KVM_CREATE_VM",
        )?;
        Ok(VmFd::new(unsafe { OwnedFd::from_raw_fd(raw) }, self.vcpu_mmap_size))
    }
}

#[cfg(test)]
mod tests {
    use crate::kvm::test_helpers::open_or_skip;

    #[test]
    fn opens_dev_kvm() {
        let Some(_) = open_or_skip("opens_dev_kvm") else { return; };
    }

    #[test]
    fn creates_vm() {
        let Some(kvm) = open_or_skip("creates_vm") else { return; };
        let _vm = kvm.create_vm().expect("create_vm");
    }
}
