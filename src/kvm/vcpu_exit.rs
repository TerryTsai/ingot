//! Parsed shape of a vCPU exit reason and the kvm_run union payload
//! that accompanies it. Decoders cover the exits handled in Phase 2:
//! halt, shutdown, MMIO, and port-I/O.

use kvm_bindings::{kvm_run, KVM_EXIT_HLT, KVM_EXIT_IO, KVM_EXIT_MMIO, KVM_EXIT_SHUTDOWN};

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
    pub(super) fn from_kvm_run(run: &kvm_run, run_ptr: *const kvm_run) -> Self {
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
