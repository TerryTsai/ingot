//! VcpuFd: a single vCPU. Owns the vCPU file descriptor and the kvm_run mmap.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use kvm_bindings::{kvm_regs, kvm_run, kvm_sregs};

use super::exit::{decode_exit, VcpuExit};
use super::ioctls::*;
use super::{check, errno, KvmError};

pub struct VcpuFd {
    fd: OwnedFd,
    id: u32,
    run_ptr: *mut kvm_run,
    mmap_size: usize,
}

impl VcpuFd {
    pub(super) fn create(vm_fd: RawFd, id: u32, mmap_size: usize) -> Result<Self, KvmError> {
        let raw = check(
            unsafe { libc::ioctl(vm_fd, KVM_CREATE_VCPU, id as libc::c_ulong) },
            "KVM_CREATE_VCPU",
        )?;
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                mmap_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(KvmError::Mmap(errno()));
        }
        Ok(VcpuFd {
            fd,
            id,
            run_ptr: p as *mut kvm_run,
            mmap_size,
        })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn get_regs(&self) -> Result<kvm_regs, KvmError> {
        let mut regs = unsafe { std::mem::zeroed::<kvm_regs>() };
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_GET_REGS, &mut regs) },
            "KVM_GET_REGS",
        )?;
        Ok(regs)
    }

    pub fn set_regs(&self, regs: &kvm_regs) -> Result<(), KvmError> {
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_REGS, regs) },
            "KVM_SET_REGS",
        )?;
        Ok(())
    }

    pub fn get_sregs(&self) -> Result<kvm_sregs, KvmError> {
        let mut sregs = unsafe { std::mem::zeroed::<kvm_sregs>() };
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_GET_SREGS, &mut sregs) },
            "KVM_GET_SREGS",
        )?;
        Ok(sregs)
    }

    pub fn set_sregs(&self, sregs: &kvm_sregs) -> Result<(), KvmError> {
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_SREGS, sregs) },
            "KVM_SET_SREGS",
        )?;
        Ok(())
    }

    pub fn run(&mut self) -> Result<VcpuExit, KvmError> {
        check(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_RUN, 0) },
            "KVM_RUN",
        )?;
        Ok(decode_exit(unsafe { &*self.run_ptr }, self.run_ptr))
    }

    pub fn set_mmio_read_data(&mut self, data: &[u8]) {
        let run = unsafe { &mut *self.run_ptr };
        let mmio = unsafe { &mut run.__bindgen_anon_1.mmio };
        let n = data.len().min(8);
        mmio.data[..n].copy_from_slice(&data[..n]);
    }
}

impl Drop for VcpuFd {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.run_ptr as _, self.mmap_size);
        }
    }
}
