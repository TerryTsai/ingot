//! Run-to-completion loop: load kernel, prime vCPU, dispatch exits.

use std::time::Instant;

use crate::boot;
use crate::error::{HostError, IngotError};
use crate::guest_mem::GuestMem;
use crate::kvm::{KvmError, VcpuExit};
use crate::outcome::{ExitStatus, GuestFaultReason, Outcome, TransitionEvent};

use super::runtime::Runtime;
use super::state::{transition, State};

const CMDLINE_ADDR: u64 = 0x2_0000;
const BOOT_PARAMS_ADDR: u64 = 0x7000;
const CMDLINE_MAX: u32 = 2048;

pub(super) fn run(mut rt: Runtime) -> Result<Outcome, IngotError> {
    step(&mut rt, State::Initializing)?;
    boot_kernel(&mut rt)?;
    step(&mut rt, State::Booting)?;
    rt.telemetry.boot_completed_at = Some(Instant::now());
    step(&mut rt, State::Running)?;
    let exit_status = run_loop(&mut rt)?;
    step(&mut rt, State::Exiting)?;
    rt.telemetry.terminated_at = Some(Instant::now());
    step(&mut rt, State::Terminated)?;
    let stdout = rt.captured_stdout.as_ref().map(|b| b.lock().unwrap().clone()).unwrap_or_default();
    Ok(Outcome { exit_status, telemetry: rt.telemetry, stdout })
}

fn step(rt: &mut Runtime, to: State) -> Result<(), IngotError> {
    let new = transition(rt.state, to)
        .map_err(|_| IngotError::Host(HostError::InvalidConfig("illegal state transition")))?;
    rt.telemetry.transitions.push(TransitionEvent { at: Instant::now(), from: rt.state, to: new });
    rt.state = new;
    Ok(())
}

fn boot_kernel(rt: &mut Runtime) -> Result<(), IngotError> {
    let kernel_bytes = std::fs::read(&rt.config.kernel_path).map_err(|_| {
        IngotError::Host(HostError::KernelImageNotReadable(rt.config.kernel_path.clone()))
    })?;
    let kernel = boot::load_kernel(&kernel_bytes, &mut rt.mem)?;
    let initramfs = load_optional_initramfs(rt, &kernel)?;
    write_cmdline(&mut rt.mem, &rt.config.cmdline)?;
    let bp = boot::build_boot_params(initramfs.as_ref(), CMDLINE_ADDR, CMDLINE_MAX, rt.config.memory_bytes)?;
    bp.write_to(BOOT_PARAMS_ADDR, &mut rt.mem)?;
    let state = boot::prime_vcpu(&kernel, BOOT_PARAMS_ADDR, &mut rt.mem)?;
    rt.vcpus[0].set_sregs(&state.sregs)?;
    rt.vcpus[0].set_regs(&state.regs)?;
    Ok(())
}

fn load_optional_initramfs(
    rt: &mut Runtime,
    kernel: &boot::KernelImage,
) -> Result<Option<boot::InitramfsImage>, IngotError> {
    let Some(p) = rt.config.initramfs_path.clone() else { return Ok(None); };
    let bytes = std::fs::read(&p).map_err(|_| IngotError::Host(HostError::InitramfsNotReadable(p)))?;
    Ok(Some(boot::load_initramfs(&bytes, &mut rt.mem, kernel)?))
}

fn write_cmdline(mem: &mut GuestMem, cmdline: &str) -> Result<(), IngotError> {
    mem.write(CMDLINE_ADDR, cmdline.as_bytes())?;
    mem.write(CMDLINE_ADDR + cmdline.len() as u64, &[0])?;
    Ok(())
}

fn run_loop(rt: &mut Runtime) -> Result<ExitStatus, IngotError> {
    loop {
        match rt.vcpus[0].run()? {
            VcpuExit::Hlt => return Ok(ExitStatus::Halted),
            VcpuExit::Shutdown => return Ok(ExitStatus::GuestFault(GuestFaultReason::TripleFault)),
            VcpuExit::MmioRead { addr, len } => handle_mmio_read(rt, addr, len)?,
            VcpuExit::MmioWrite { addr, data, len } => rt.bus.write(addr, &data[..len])?,
            VcpuExit::IoIn { len, .. } => handle_pio_in(rt, len),
            VcpuExit::IoOut { .. } => {}
            VcpuExit::Other(r) => return Err(IngotError::Kvm(KvmError::UnhandledExit(r))),
        }
    }
}

fn handle_mmio_read(rt: &mut Runtime, addr: u64, len: usize) -> Result<(), IngotError> {
    let mut buf = [0u8; 8];
    rt.bus.read(addr, &mut buf[..len])?;
    rt.vcpus[0].set_mmio_read_data(&buf[..len]);
    Ok(())
}

/// Unhandled PIO read: return 0xFF for each byte (the "no device
/// present" convention that lets the kernel proceed without thinking
/// it found something at the port).
fn handle_pio_in(rt: &mut Runtime, len: usize) {
    let bytes = [0xFFu8; 4];
    rt.vcpus[0].set_pio_read_data(&bytes[..len.min(4)]);
}
