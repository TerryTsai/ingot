//! Handle to a KVM virtual machine. Registers guest memory, creates
//! vCPUs, enables the in-kernel IRQ chip.

use std::os::fd::{AsRawFd, OwnedFd};

use kvm_bindings::kvm_userspace_memory_region;

use super::ioctls::*;
use super::vcpu::Vcpu;
use super::KvmError;

pub(crate) struct Vm {
    fd: OwnedFd,
    vcpu_mmap_size: usize,
}

impl Vm {
    pub(super) fn new(fd: OwnedFd, vcpu_mmap_size: usize) -> Self {
        Vm { fd, vcpu_mmap_size }
    }

    /// Sets the physical base address of KVM's 3-page TSS region. Required on
    /// Intel hosts before `create_irqchip`. The region must not overlap any
    /// registered userspace memory region.
    pub fn set_tss_addr(&self, addr: u32) -> Result<(), KvmError> {
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_TSS_ADDR, addr as libc::c_ulong) },
            "KVM_SET_TSS_ADDR",
        )?;
        Ok(())
    }

    /// Enables KVM's in-kernel emulation of PIC, IOAPIC, and per-vCPU LAPIC.
    /// Must be called before any vCPU is created and (on Intel) after
    /// `set_tss_addr`.
    pub fn create_irqchip(&self) -> Result<(), KvmError> {
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_IRQCHIP, 0) },
            "KVM_CREATE_IRQCHIP",
        )?;
        Ok(())
    }

    /// Enables KVM's in-kernel 8253 PIT emulation. Linux uses PIT for early
    /// timer calibration; without it the kernel hangs after init_IRQ.
    pub fn create_pit2(&self) -> Result<(), KvmError> {
        let config = [0u32; 16]; // struct kvm_pit_config { flags=0, pad[15]=0 }
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_CREATE_PIT2, &config) },
            "KVM_CREATE_PIT2",
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
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_USER_MEMORY_REGION, &region) },
            "KVM_SET_USER_MEMORY_REGION",
        )?;
        Ok(())
    }

    pub fn create_vcpu(&self, id: u32) -> Result<Vcpu, KvmError> {
        Vcpu::create(self.fd.as_raw_fd(), id, self.vcpu_mmap_size)
    }

    /// Set GSI line `irq` to `level` (1 = asserted, 0 = deasserted) on the
    /// in-kernel IRQ chip. Edge-triggered ISA IRQs need a 0→1 pulse;
    /// level-triggered devices set 1 and clear 0 when the source clears.
    pub fn set_irq_line(&self, irq: u32, level: u32) -> Result<(), KvmError> {
        let payload = [irq, level];
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_IRQ_LINE, &payload) },
            "KVM_IRQ_LINE",
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::kvm::Kvm;

    #[test]
    fn creates_vcpu() {
        let Some(kvm) = Kvm::open_or_skip("creates_vcpu") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        let vcpu = vm.create_vcpu(0).expect("create_vcpu");
        assert_eq!(vcpu.id(), 0);
    }

    #[test]
    fn creates_irqchip() {
        let Some(kvm) = Kvm::open_or_skip("creates_irqchip") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        vm.create_irqchip().expect("create_irqchip");
    }
}
