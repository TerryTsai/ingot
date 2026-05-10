//! Runtime: KVM, guest memory, vCPUs, MMIO bus, telemetry. Constructed
//! by `Vm::new`; consumed by `lifecycle::run`.

use std::time::Instant;

use crate::config::Config;
use crate::devices::{MmioBus, Uart};
use crate::error::IngotError;
use crate::guest_mem::GuestMem;
use crate::kvm::{Kvm, VcpuFd, VmFd};
use crate::outcome::Telemetry;

use super::sink::{make_sink, maybe_spawn_stdin, Captured};
use super::state::State;
use super::validate::validate;

pub(crate) const UART_BASE: u64 = 0xD000_0000;
pub(crate) const UART_SIZE: u64 = 0x1000;

pub(crate) struct Runtime {
    pub config: Config,
    pub state: State,
    pub telemetry: Telemetry,
    pub bus: MmioBus,
    pub vcpus: Vec<VcpuFd>,
    pub vm_fd: VmFd,
    pub mem: GuestMem,
    pub captured_stdout: Captured,
    _kvm: Kvm,
}

impl Runtime {
    pub fn new(config: Config) -> Result<Self, IngotError> {
        validate(&config)?;
        let kvm = Kvm::open()?;
        let vm_fd = kvm.create_vm()?;
        let mem = GuestMem::new(config.memory_bytes)?;
        vm_fd.set_user_memory_region(0, 0, mem.host_ptr(), config.memory_bytes)?;
        let vcpus = make_vcpus(&vm_fd, config.vcpus)?;
        let (bus, captured_stdout) = make_bus(&config)?;
        Ok(Runtime {
            telemetry: fresh_telemetry(),
            state: State::Configured,
            bus,
            vcpus,
            vm_fd,
            mem,
            captured_stdout,
            config,
            _kvm: kvm,
        })
    }
}

fn make_vcpus(vm: &VmFd, count: u8) -> Result<Vec<VcpuFd>, IngotError> {
    let mut v = Vec::with_capacity(count as usize);
    for i in 0..count {
        v.push(vm.create_vcpu(i as u32)?);
    }
    Ok(v)
}

fn make_bus(config: &Config) -> Result<(MmioBus, Captured), IngotError> {
    let mut bus = MmioBus::new();
    let (sink, captured) = make_sink(&config.stdout)?;
    let uart = Uart::new(sink);
    maybe_spawn_stdin(uart.input_handle(), config.interactive);
    bus.register(UART_BASE, UART_SIZE, Box::new(uart))?;
    Ok((bus, captured))
}

fn fresh_telemetry() -> Telemetry {
    Telemetry {
        boot_started_at: Instant::now(),
        boot_completed_at: None,
        terminated_at: None,
        transitions: Vec::new(),
        peak_rss_kb: None,
    }
}
