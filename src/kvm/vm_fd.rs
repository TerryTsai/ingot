//! Handle to a KVM virtual machine. Registers guest memory, creates
//! vCPUs, enables the in-kernel IRQ chip.

use std::os::fd::{AsRawFd, OwnedFd};

use kvm_bindings::kvm_userspace_memory_region;

use super::ioctls::*;
use super::vcpu_fd::VcpuFd;
use super::{check, KvmError};

pub(crate) struct VmFd {
    fd: OwnedFd,
    vcpu_mmap_size: usize,
}

impl VmFd {
    pub(super) fn new(fd: OwnedFd, vcpu_mmap_size: usize) -> Self {
        VmFd { fd, vcpu_mmap_size }
    }

    /// Enables KVM's in-kernel emulation of PIC, IOAPIC, and per-vCPU LAPIC.
    /// Must be called before any vCPU is created.
    pub fn create_irqchip(&self) -> Result<(), KvmError> {
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_IRQCHIP, 0) },
            "KVM_CREATE_IRQCHIP",
        )?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use crate::kvm::test_helpers::open_or_skip;

    #[test]
    fn creates_vcpu() {
        let Some(kvm) = open_or_skip("creates_vcpu") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        let vcpu = vm.create_vcpu(0).expect("create_vcpu");
        assert_eq!(vcpu.id(), 0);
    }

    #[test]
    fn creates_irqchip() {
        let Some(kvm) = open_or_skip("creates_irqchip") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        vm.create_irqchip().expect("create_irqchip");
    }
}
