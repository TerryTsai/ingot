//! Handle to /dev/kvm. Opens the device, validates the ABI version,
//! and creates VMs.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use kvm_bindings::KVM_API_VERSION;

use super::cpuid::Cpuid;
use super::ioctls::*;
use super::vm_fd::VmFd;
use super::KvmError;

const DEV_KVM: &[u8] = b"/dev/kvm\0";

pub(crate) struct KvmFd {
    fd: OwnedFd,
    pub(super) vcpu_mmap_size: usize,
}

impl KvmFd {
    pub fn open() -> Result<Self, KvmError> {
        let raw = unsafe { libc::open(DEV_KVM.as_ptr() as _, libc::O_RDWR | libc::O_CLOEXEC) };
        if raw < 0 {
            return Err(KvmError::Open(KvmError::last_errno()));
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let api = KvmError::check_ioctl(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_API_VERSION, 0) },
            "KVM_GET_API_VERSION",
        )?;
        if api as u32 != KVM_API_VERSION {
            return Err(KvmError::AbiVersionMismatch { got: api, want: KVM_API_VERSION });
        }
        let mmap = KvmError::check_ioctl(
            unsafe { libc::ioctl(fd.as_raw_fd(), KVM_GET_VCPU_MMAP_SIZE, 0) },
            "KVM_GET_VCPU_MMAP_SIZE",
        )?;
        Ok(KvmFd { fd, vcpu_mmap_size: mmap as usize })
    }

    pub fn create_vm(&self) -> Result<VmFd, KvmError> {
        let raw = KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_VM, 0) },
            "KVM_CREATE_VM",
        )?;
        Ok(VmFd::new(unsafe { OwnedFd::from_raw_fd(raw) }, self.vcpu_mmap_size))
    }

    /// Returns the CPUID entries KVM will honor when set on a vCPU.
    pub fn supported_cpuid(&self) -> Result<Box<Cpuid>, KvmError> {
        let mut buf = Cpuid::new();
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_GET_SUPPORTED_CPUID, &mut *buf) },
            "KVM_GET_SUPPORTED_CPUID",
        )?;
        Ok(buf)
    }

    /// Test-only: try to open `/dev/kvm`, return `None` and log a skip
    /// message if unavailable. Shared by every test that needs a real
    /// KVM handle — keeps the test suite passing in environments
    /// without KVM (CI sandboxes, containers without device passthrough).
    #[cfg(test)]
    pub(crate) fn open_or_skip(test_name: &str) -> Option<Self> {
        match Self::open() {
            Ok(k) => Some(k),
            Err(e) => {
                eprintln!("[{}] skipped: cannot open /dev/kvm ({:?})", test_name, e);
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_dev_kvm() {
        let Some(_) = KvmFd::open_or_skip("opens_dev_kvm") else { return; };
    }

    #[test]
    fn creates_vm() {
        let Some(kvm) = KvmFd::open_or_skip("creates_vm") else { return; };
        let _vm = kvm.create_vm().expect("create_vm");
    }
}
