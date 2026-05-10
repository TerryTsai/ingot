//! VmFd: a single KVM VM. Owns the VM file descriptor.

use std::os::fd::{AsRawFd, OwnedFd};

use kvm_bindings::kvm_userspace_memory_region;

use super::ioctls::*;
use super::{check, KvmError, VcpuFd};

pub struct VmFd {
    fd: OwnedFd,
    vcpu_mmap_size: usize,
}

impl VmFd {
    pub(super) fn new(fd: OwnedFd, vcpu_mmap_size: usize) -> Self {
        VmFd { fd, vcpu_mmap_size }
    }

    pub fn set_user_memory_region(
        &self,
        slot: u32,
        guest_phys: u64,
        host_addr: *mut u8,
        size: u64,
    ) -> Result<(), KvmError> {
        let region = kvm_userspace_memory_region {
            slot,
            flags: 0,
            guest_phys_addr: guest_phys,
            memory_size: size,
            userspace_addr: host_addr as u64,
        };
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_USER_MEMORY_REGION, &region) },
            "KVM_SET_USER_MEMORY_REGION",
        )?;
        Ok(())
    }

    pub fn create_vcpu(&self, id: u32) -> Result<VcpuFd, KvmError> {
        VcpuFd::create(self.fd.as_raw_fd(), id, self.vcpu_mmap_size)
    }
}
