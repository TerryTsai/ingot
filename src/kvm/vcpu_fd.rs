//! Handle to a KVM vCPU. Wraps the per-vCPU file descriptor and the
//! mmap'd kvm_run structure used to exchange entry/exit state.
//! `VcpuExit` is the typed return shape of `VcpuFd::run` — produced
//! only here, lives next to its producer.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use kvm_bindings::{
    kvm_regs, kvm_run, kvm_sregs, KVM_EXIT_HLT, KVM_EXIT_IO, KVM_EXIT_MMIO, KVM_EXIT_SHUTDOWN,
};

use super::cpuid::Cpuid;
use super::ioctls::*;
use super::KvmError;

#[derive(Debug)]
pub enum VcpuExit {
    Hlt,
    Shutdown,
    MmioRead { addr: u64, len: usize },
    MmioWrite { addr: u64, data: [u8; 8], len: usize },
    IoIn { port: u16, len: usize },
    IoOut { port: u16, data: [u8; 4], len: usize },
    /// KVM_RUN was interrupted by a signal (we use SIGALRM to wake from
    /// hlt periodically so we can poll host fds like TAP). Run-loop
    /// treats this as a no-op exit.
    Intr,
    Other(u32),
}

impl VcpuExit {
    /// Decode the kvm_run union after a `KVM_RUN` return. `run_ptr` is the
    /// base of the mmap'd kvm_run page; we need it (not just `run`) because
    /// IoOut's payload lives at `run + data_offset` rather than inline in
    /// the union.
    fn from_kvm_run(run: &kvm_run, run_ptr: *const kvm_run) -> Self {
        match run.exit_reason {
            KVM_EXIT_HLT => VcpuExit::Hlt,
            KVM_EXIT_SHUTDOWN => VcpuExit::Shutdown,
            KVM_EXIT_MMIO => Self::decode_mmio(unsafe { &run.__bindgen_anon_1.mmio }),
            KVM_EXIT_IO => Self::decode_io(run_ptr as *const u8, unsafe { &run.__bindgen_anon_1.io }),
            other => VcpuExit::Other(other),
        }
    }

    fn decode_mmio(m: &kvm_bindings::kvm_run__bindgen_ty_1__bindgen_ty_6) -> Self {
        let n = (m.len as usize).min(8);
        if m.is_write != 0 {
            let mut data = [0u8; 8];
            data[..n].copy_from_slice(&m.data[..n]);
            VcpuExit::MmioWrite { addr: m.phys_addr, data, len: n }
        } else {
            VcpuExit::MmioRead { addr: m.phys_addr, len: n }
        }
    }

    fn decode_io(run_base: *const u8, io: &kvm_bindings::kvm_run__bindgen_ty_1__bindgen_ty_4) -> Self {
        let n = (io.size as usize).min(4);
        if io.direction == 1 {
            let mut data = [0u8; 4];
            let src = unsafe { std::slice::from_raw_parts(run_base.add(io.data_offset as usize), n) };
            data[..n].copy_from_slice(src);
            VcpuExit::IoOut { port: io.port, data, len: n }
        } else {
            VcpuExit::IoIn { port: io.port, len: n }
        }
    }
}

pub(crate) struct VcpuFd {
    fd: OwnedFd,
    id: u32,
    run_ptr: *mut kvm_run,
    mmap_size: usize,
}

impl VcpuFd {
    pub(super) fn create(vm_fd: RawFd, id: u32, mmap_size: usize) -> Result<Self, KvmError> {
        let raw = KvmError::check_ioctl(
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
            return Err(KvmError::Mmap(KvmError::last_errno()));
        }
        Ok(VcpuFd { fd, id, run_ptr: p as *mut kvm_run, mmap_size })
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn get_regs(&self) -> Result<kvm_regs, KvmError> {
        let mut regs = unsafe { std::mem::zeroed::<kvm_regs>() };
        KvmError::check_ioctl(unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_GET_REGS, &mut regs) }, "KVM_GET_REGS")?;
        Ok(regs)
    }

    pub fn set_regs(&self, regs: &kvm_regs) -> Result<(), KvmError> {
        KvmError::check_ioctl(unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_REGS, regs) }, "KVM_SET_REGS")?;
        Ok(())
    }

    pub fn get_sregs(&self) -> Result<kvm_sregs, KvmError> {
        let mut sregs = unsafe { std::mem::zeroed::<kvm_sregs>() };
        KvmError::check_ioctl(unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_GET_SREGS, &mut sregs) }, "KVM_GET_SREGS")?;
        Ok(sregs)
    }

    pub fn set_sregs(&self, sregs: &kvm_sregs) -> Result<(), KvmError> {
        KvmError::check_ioctl(unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_SREGS, sregs) }, "KVM_SET_SREGS")?;
        Ok(())
    }

    pub fn set_cpuid2(&self, cpuid: &Cpuid) -> Result<(), KvmError> {
        KvmError::check_ioctl(
            unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_SET_CPUID2, cpuid as *const _) },
            "KVM_SET_CPUID2",
        )?;
        Ok(())
    }

    pub fn run(&mut self) -> Result<VcpuExit, KvmError> {
        let r = unsafe { libc::ioctl(self.fd.as_raw_fd(), KVM_RUN, 0) };
        if r < 0 {
            let e = KvmError::last_errno();
            if e == libc::EINTR { return Ok(VcpuExit::Intr); }
            return Err(KvmError::Ioctl { name: "KVM_RUN", errno: e });
        }
        Ok(VcpuExit::from_kvm_run(unsafe { &*self.run_ptr }, self.run_ptr))
    }

    pub fn set_mmio_read_data(&mut self, data: &[u8]) {
        let run = unsafe { &mut *self.run_ptr };
        let mmio = unsafe { &mut run.__bindgen_anon_1.mmio };
        let n = data.len().min(8);
        mmio.data[..n].copy_from_slice(&data[..n]);
    }

    pub fn set_pio_read_data(&mut self, data: &[u8]) {
        let run = unsafe { &*self.run_ptr };
        let io = unsafe { &run.__bindgen_anon_1.io };
        let dst = unsafe { (self.run_ptr as *mut u8).add(io.data_offset as usize) };
        let n = data.len().min(io.size as usize);
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), dst, n); }
    }
}

impl Drop for VcpuFd {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.run_ptr as _, self.mmap_size); }
    }
}

#[cfg(test)]
mod tests {
    use crate::kvm::KvmFd;

    #[test]
    fn gets_and_sets_regs() {
        let Some(kvm) = KvmFd::open_or_skip("gets_and_sets_regs") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        let vcpu = vm.create_vcpu(0).expect("create_vcpu");
        let mut regs = vcpu.get_regs().expect("get_regs");
        regs.rip = 0x1000;
        regs.rflags = 0x2;
        vcpu.set_regs(&regs).expect("set_regs");
        assert_eq!(vcpu.get_regs().unwrap().rip, 0x1000);
    }

    #[test]
    fn gets_and_sets_sregs() {
        let Some(kvm) = KvmFd::open_or_skip("gets_and_sets_sregs") else { return; };
        let vm = kvm.create_vm().expect("create_vm");
        let vcpu = vm.create_vcpu(0).expect("create_vcpu");
        let mut sregs = vcpu.get_sregs().expect("get_sregs");
        sregs.cs.base = 0;
        sregs.cs.selector = 0;
        vcpu.set_sregs(&sregs).expect("set_sregs");
    }

    /// 4 KiB real-mode hello world: the Phase 1 done-criterion test.
    /// Maps a single page containing one `hlt` (0xF4) at guest physical
    /// 0x1000, sets CS:IP = 0:0x1000, runs, expects KVM_EXIT_HLT.
    #[test]
    fn real_mode_hlt() {
        use crate::kvm::VcpuExit;
        const PAGE_SIZE: usize = 4096;
        const CODE_GUEST_PHYS: u64 = 0x1000;

        let Some(kvm) = KvmFd::open_or_skip("real_mode_hlt") else { return; };
        let vm = kvm.create_vm().expect("create_vm");

        let page = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                PAGE_SIZE,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(page, libc::MAP_FAILED, "mmap host page");
        unsafe { *(page as *mut u8) = 0xf4; }

        vm.set_user_memory_region(0, CODE_GUEST_PHYS, page as *mut u8, PAGE_SIZE as u64)
            .expect("set_user_memory_region");

        let mut vcpu = vm.create_vcpu(0).expect("create_vcpu");
        let mut sregs = vcpu.get_sregs().expect("get_sregs");
        sregs.cs.base = 0;
        sregs.cs.selector = 0;
        vcpu.set_sregs(&sregs).expect("set_sregs");

        let mut regs = vcpu.get_regs().expect("get_regs");
        regs.rip = CODE_GUEST_PHYS;
        regs.rflags = 0x2;
        vcpu.set_regs(&regs).expect("set_regs");

        match vcpu.run().expect("run") {
            VcpuExit::Hlt => {}
            other => panic!("expected Hlt, got {:?}", other),
        }

        unsafe { libc::munmap(page, PAGE_SIZE); }
    }
}
