//! KVM ioctl wrapper.
//!
//! Phase 1 deliverable. Currently a sketch.

use std::os::unix::io::RawFd;

pub struct Kvm {
    fd: RawFd,
}

pub struct VmFd {
    fd: RawFd,
}

pub struct VcpuFd {
    fd: RawFd,
    id: u32,
}

pub enum VcpuExit {
    Hlt,
    Shutdown,
    MmioRead { addr: u64, len: usize },
    MmioWrite { addr: u64, data: [u8; 8], len: usize },
    IoIn { port: u16, len: usize },
    IoOut { port: u16, data: [u8; 4], len: usize },
    Other(u32),
}

pub struct RegState {
    pub rip: u64,
    pub rsp: u64,
    pub cr3: u64,
    pub rsi: u64,
}

pub enum KvmError {
    OpenDevKvm(i32),
    AbiVersionMismatch { got: u32, want: u32 },
    CreateVm(i32),
    CreateVcpu(i32),
    SetMemoryRegion(i32),
    SetRegs(i32),
    SetSregs(i32),
    Run(i32),
    UnhandledExit(u32),
    InternalError,
}

impl Kvm {
    pub fn open() -> Result<Self, KvmError> {
        unimplemented!()
    }

    pub fn create_vm(&self) -> Result<VmFd, KvmError> {
        unimplemented!()
    }
}

impl VmFd {
    pub fn map_memory(
        &self,
        slot: u32,
        guest_phys: u64,
        host_addr: *mut u8,
        size: u64,
    ) -> Result<(), KvmError> {
        unimplemented!()
    }

    pub fn create_vcpu(&self, id: u32) -> Result<VcpuFd, KvmError> {
        unimplemented!()
    }
}

impl VcpuFd {
    pub fn set_regs(&self, regs: &RegState) -> Result<(), KvmError> {
        unimplemented!()
    }

    pub fn run(&mut self) -> Result<VcpuExit, KvmError> {
        unimplemented!()
    }
}
