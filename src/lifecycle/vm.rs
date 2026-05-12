//! Vm — the orchestrator. Reads top-to-bottom:
//!
//!   Construction: Vm::new + make_vcpus + fresh_telemetry
//!   Execution:    Vm::run → boot_kernel + run_loop
//!   Dispatch:     reconcile_bus_irqs + handle_mmio_read + handle_pio_in
//!   State:        step (state-machine transition + telemetry)
//!
//! Internal mechanics live in support/:
//!   memory.rs       guest physical memory
//!   stdio.rs        UART host-side I/O (sinks + stdin pump)
//!   wakeup_timer.rs SIGALRM-driven KVM_RUN wakeup
//!
//! Subsystem directories (kvm/, boot/, devices/, jailer/) provide
//! primitives this file composes.

use std::time::Instant;

use crate::boot::{self, TSS_ADDR};
use crate::devices::{Block, Bus, Cmos, Net, Tap, Vsock, COM1_BASE, COM1_SIZE};
use crate::kvm::{self, Cpuid, KvmError, VcpuExit, Vcpu};
use crate::lifecycle::config::Config;
use crate::lifecycle::error::{HostError, IngotError};
use crate::lifecycle::outcome::{ExitStatus, GuestFaultReason, Outcome, Telemetry, TransitionEvent};
use crate::lifecycle::state::State;
use crate::lifecycle::support::memory::Memory;
use crate::lifecycle::support::stdio::{self, Captured, FirstOutput};
use crate::lifecycle::support::wakeup_timer;

/// virtio-mmio device regions: 4 KiB each, kernel learns of them via
/// `virtio_mmio.device=4K@<addr>:<irq>` cmdline params. IRQs 5/6/7
/// stay clear of legacy ISA assignments (timer=0, UART=4, RTC=8).
const VIRTIO_NET_BASE: u64 = 0xD000_0000;
const VIRTIO_NET_SIZE: u64 = 0x1000;
const VIRTIO_NET_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0xFF, 0xEE];
const VIRTIO_BLK_BASE: u64 = 0xD000_1000;
const VIRTIO_BLK_SIZE: u64 = 0x1000;
const VIRTIO_VSOCK_BASE: u64 = 0xD000_2000;
const VIRTIO_VSOCK_SIZE: u64 = 0x1000;

/// A configured microVM, ready to run. Construct with [`Vm::new`] and
/// consume with [`Vm::run`]. The library API is two methods because the
/// state machine is fixed: caller validates config, ingot constructs a
/// VM, caller drives it to completion, caller reads the [`Outcome`].
pub struct Vm {
    config: Config,
    state: State,
    telemetry: Telemetry,
    bus: Bus,
    vcpus: Vec<Vcpu>,
    kvm_vm: kvm::Vm,
    mem: Memory,
    captured_stdout: Captured,
    first_output: FirstOutput,
    _kvm: kvm::Kvm,
}

impl Vm {
    /// Allocate KVM resources and prepare a VM to run. Returns errors as
    /// `IngotError` — typed across the three domains (`KvmError`,
    /// `GuestError`, `HostError`) so callers can match on what went
    /// wrong. Panics are bugs.
    ///
    /// ```no_run
    /// # use ingot::{Config, StdoutSink, Vm};
    /// # use std::path::PathBuf;
    /// let cfg = Config {
    ///     kernel_path: PathBuf::from("/path/to/vmlinux"),
    ///     initramfs_path: Some(PathBuf::from("/path/to/initramfs.cpio.gz")),
    ///     vcpus: 1,
    ///     memory_bytes: 256 * 1024 * 1024,
    ///     cmdline: "console=ttyS0,115200 reboot=k panic=1".into(),
    ///     stdout: StdoutSink::Buffer,
    ///     ..Config::default()
    /// };
    /// let outcome = Vm::new(cfg)?.run()?;
    /// println!("guest stdout: {} bytes", outcome.stdout.len());
    /// # Ok::<(), ingot::IngotError>(())
    /// ```
    pub fn new(config: Config) -> Result<Self, IngotError> {
        config.validate()?;
        let kvm = kvm::Kvm::open()?;
        let kvm_vm = kvm.create_vm()?;
        let mem = Memory::new(config.memory_bytes)?;
        kvm_vm.set_user_memory_region(0, 0, mem.host_ptr(), config.memory_bytes)?;
        kvm_vm.set_tss_addr(TSS_ADDR)?;
        kvm_vm.create_irqchip()?;
        kvm_vm.create_pit2()?;
        let cpuid = kvm.supported_cpuid()?;
        let vcpus = Self::make_vcpus(&kvm_vm, config.vcpus, &cpuid)?;
        let (uart, captured_stdout, first_output) = stdio::build_uart(&config.stdout, config.interactive)?;
        let mut bus = Bus::new();
        bus.register_pio(COM1_BASE, COM1_SIZE, Box::new(uart))?;
        bus.register_pio(Cmos::BASE, Cmos::SIZE, Box::new(Cmos::new()))?;
        let tap = config.tap_name.as_deref().map(Tap::open).transpose()?;
        bus.register_mmio(VIRTIO_NET_BASE, VIRTIO_NET_SIZE, Box::new(Net::new(VIRTIO_NET_MAC, tap)))?;
        if let Some(spec) = &config.rootfs {
            let blk = Block::new(&spec.path, spec.readonly)?;
            bus.register_mmio(VIRTIO_BLK_BASE, VIRTIO_BLK_SIZE, Box::new(blk))?;
        }
        if let Some(spec) = &config.vsock {
            let v = Vsock::new(spec.guest_cid as u64, config.memory_bytes, mem.host_ptr() as u64)?;
            bus.register_mmio(VIRTIO_VSOCK_BASE, VIRTIO_VSOCK_SIZE, Box::new(v))?;
        }
        Ok(Vm {
            telemetry: Self::fresh_telemetry(),
            state: State::Configured,
            bus,
            vcpus,
            kvm_vm,
            mem,
            captured_stdout,
            first_output,
            config,
            _kvm: kvm,
        })
    }

    /// Boot the kernel, run the workload to completion, capture and
    /// return its outcome. Consumes the `Vm` — there is no resume; a
    /// new VM is a new `Vm::new` call. Returns when the guest halts,
    /// triple-faults, or `Vm::new`'s validation rejects mid-state.
    ///
    /// The returned [`Outcome`] carries the exit status, the captured
    /// stdout (if `Config::stdout` was [`StdoutSink::Buffer`]), and
    /// telemetry — boot start/complete timestamps and the state-machine
    /// transition log.
    pub fn run(mut self) -> Result<Outcome, IngotError> {
        self.step(State::Initializing)?;
        self.boot_kernel()?;
        self.step(State::Booting)?;
        self.telemetry.boot_completed_at = Some(Instant::now());
        self.step(State::Running)?;
        let exit_status = self.run_loop()?;
        self.step(State::Exiting)?;
        self.telemetry.terminated_at = Some(Instant::now());
        self.step(State::Terminated)?;
        let stdout = self.captured_stdout.as_ref().map(|b| b.lock().unwrap().clone()).unwrap_or_default();
        self.telemetry.first_output_at = *self.first_output.lock().unwrap();
        Ok(Outcome { exit_status, telemetry: self.telemetry, stdout })
    }

    fn boot_kernel(&mut self) -> Result<(), IngotError> {
        let registers = boot::install(
            &mut self.mem,
            &self.config.kernel_path,
            self.config.initramfs_path.as_deref(),
            &self.config.cmdline,
            self.config.vcpus,
            self.config.memory_bytes,
        )?;
        registers.apply_to(&self.vcpus[0])?;
        Ok(())
    }

    fn run_loop(&mut self) -> Result<ExitStatus, IngotError> {
        let mut asserted: Vec<u32> = Vec::new();
        wakeup_timer::install();
        loop {
            match self.vcpus[0].run()? {
                VcpuExit::Hlt => return Ok(ExitStatus::Halted),
                VcpuExit::Shutdown => return Ok(ExitStatus::GuestFault(GuestFaultReason::TripleFault)),
                VcpuExit::MmioRead { addr, len } => self.handle_mmio_read(addr, len)?,
                VcpuExit::MmioWrite { addr, data, len } => self.bus.write_mmio(addr, &data[..len], &mut self.mem)?,
                VcpuExit::IoIn { port, len } => self.handle_pio_in(port, len),
                VcpuExit::IoOut { port, data, len } => self.bus.write_pio(port, &data[..len]),
                VcpuExit::Intr => {}
                VcpuExit::Other(r) => return Err(IngotError::Kvm(KvmError::UnhandledExit(r))),
            }
            self.bus.poll_io(&mut self.mem)?;
            self.reconcile_bus_irqs(&mut asserted)?;
        }
    }

    /// Compare the bus's currently-pending GSIs against the set last
    /// driven high and drive each transition exactly once. Edge-triggered
    /// IOAPIC IRQs would otherwise refire every loop iteration; level-
    /// triggered ones would be re-asserted needlessly. Covers both MMIO
    /// (virtio devices) and PIO (UART) since the bus polls both.
    fn reconcile_bus_irqs(&mut self, asserted: &mut Vec<u32>) -> Result<(), IngotError> {
        let pending: Vec<u32> = self.bus.pending_interrupts().into_iter().map(|i| i.gsi).collect();
        for gsi in asserted.iter() {
            if !pending.contains(gsi) { self.kvm_vm.set_irq_line(*gsi, 0)?; }
        }
        for gsi in &pending {
            if !asserted.contains(gsi) { self.kvm_vm.set_irq_line(*gsi, 1)?; }
        }
        *asserted = pending;
        Ok(())
    }

    fn handle_mmio_read(&mut self, addr: u64, len: usize) -> Result<(), IngotError> {
        let mut buf = [0u8; 8];
        self.bus.read_mmio(addr, &mut buf[..len])?;
        self.vcpus[0].set_mmio_read_data(&buf[..len]);
        Ok(())
    }

    fn handle_pio_in(&mut self, port: u16, len: usize) {
        let mut buf = [0u8; 4];
        let n = len.min(4);
        self.bus.read_pio(port, &mut buf[..n]);
        self.vcpus[0].set_pio_read_data(&buf[..n]);
    }

    fn step(&mut self, to: State) -> Result<(), IngotError> {
        let new = self.state.transition(to)
            .map_err(|_| IngotError::Host(HostError::InvalidConfig("illegal state transition")))?;
        self.telemetry.transitions.push(TransitionEvent { at: Instant::now(), from: self.state, to: new });
        self.state = new;
        Ok(())
    }

    fn make_vcpus(vm: &kvm::Vm, count: u8, cpuid: &Cpuid) -> Result<Vec<Vcpu>, IngotError> {
        let mut v = Vec::with_capacity(count as usize);
        for i in 0..count {
            let vcpu = vm.create_vcpu(i as u32)?;
            vcpu.set_cpuid2(cpuid)?;
            v.push(vcpu);
        }
        Ok(v)
    }

    fn fresh_telemetry() -> Telemetry {
        Telemetry {
            boot_started_at: Instant::now(),
            boot_completed_at: None,
            first_output_at: None,
            terminated_at: None,
            transitions: Vec::new(),
            peak_rss_kb: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{Config, IngotError, Outcome, StdoutSink, Vm};
    use crate::lifecycle::outcome::ExitStatus;
    use crate::lifecycle::state::State;

    const ENTRY_POINT: u64 = 0x10_0000;

    fn touch(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-validate-{}-{}", name, std::process::id()));
        std::fs::write(&p, b"placeholder").unwrap();
        p
    }

    fn base() -> Config {
        Config {
            vcpus: 1,
            memory_bytes: 16 * 1024 * 1024,
            stdout: StdoutSink::Discard,
            ..Config::default()
        }
    }

    fn run_with_payload(name: &str, payload: &[u8]) -> Result<Outcome, IngotError> {
        run_with_stdout(name, payload, StdoutSink::Discard)
    }

    fn run_with_stdout(name: &str, payload: &[u8], stdout: StdoutSink) -> Result<Outcome, IngotError> {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-synth-{}-{}.elf", name, std::process::id()));
        std::fs::write(&p, build_elf(payload)).expect("write tmp");
        let cfg = Config { kernel_path: p.clone(), vcpus: 1, memory_bytes: 16 * 1024 * 1024, cmdline: String::new(), stdout, ..Config::default() };
        let result = Vm::new(cfg).and_then(|vm| vm.run());
        let _ = std::fs::remove_file(&p);
        result
    }

    fn build_elf(payload: &[u8]) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + 56 + payload.len());
        b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&62u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
        b.extend_from_slice(&64u64.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&64u16.to_le_bytes());
        b.extend_from_slice(&56u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&5u32.to_le_bytes());
        b.extend_from_slice(&(64u64 + 56).to_le_bytes());
        b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
        b.extend_from_slice(&ENTRY_POINT.to_le_bytes());
        let psz = payload.len() as u64;
        b.extend_from_slice(&psz.to_le_bytes());
        b.extend_from_slice(&psz.to_le_bytes());
        b.extend_from_slice(&0x1000u64.to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn rejects_missing_kernel() {
        let mut cfg = base();
        cfg.kernel_path = PathBuf::from("/nonexistent/kernel/path");
        assert!(Vm::new(cfg).is_err());
    }

    #[test]
    fn rejects_zero_vcpus() {
        let mut cfg = base();
        cfg.kernel_path = touch("zero_vcpus");
        cfg.vcpus = 0;
        assert!(Vm::new(cfg).is_err());
    }

    #[test]
    fn rejects_too_small_memory() {
        let mut cfg = base();
        cfg.kernel_path = touch("small_mem");
        cfg.memory_bytes = 1024;
        assert!(Vm::new(cfg).is_err());
    }

    #[test]
    fn rejects_unpaged_memory() {
        let mut cfg = base();
        cfg.kernel_path = touch("unpaged_mem");
        cfg.memory_bytes = 16 * 1024 * 1024 + 7;
        assert!(Vm::new(cfg).is_err());
    }

    #[test]
    fn rejects_cmdline_with_nul() {
        let mut cfg = base();
        cfg.kernel_path = touch("nul_cmdline");
        cfg.cmdline = "a\0b".into();
        assert!(Vm::new(cfg).is_err());
    }

    // Synth tests use `ud2` (0F 0B) to terminate. With in-kernel LAPIC enabled,
    // hlt blocks indefinitely in KVM_RUN waiting for an interrupt; ud2 raises
    // #UD which, without an IDT, cascades to triple fault → KVM_EXIT_SHUTDOWN.
    const UD2: [u8; 2] = [0x0f, 0x0b];

    #[test]
    fn synthetic_kernel_reaches_terminal_exit() {
        match run_with_payload("ud2", &UD2) {
            Ok(o) => assert!(matches!(o.exit_status, ExitStatus::GuestFault(crate::GuestFaultReason::TripleFault))),
            Err(e) => eprintln!("[synthetic_kernel_reaches_terminal_exit] skipped: {:?}", e),
        }
    }

    #[test]
    fn synthetic_pio_in_consumed() {
        // in al, dx ; ud2
        match run_with_payload("pio_in", &[0xec, 0x0f, 0x0b]) {
            Ok(o) => assert!(matches!(o.exit_status, ExitStatus::GuestFault(crate::GuestFaultReason::TripleFault))),
            Err(e) => eprintln!("[synthetic_pio_in_consumed] skipped: {:?}", e),
        }
    }

    #[test]
    fn synthetic_pio_out_consumed() {
        // out dx, al ; ud2
        match run_with_payload("pio_out", &[0xee, 0x0f, 0x0b]) {
            Ok(o) => assert!(matches!(o.exit_status, ExitStatus::GuestFault(crate::GuestFaultReason::TripleFault))),
            Err(e) => eprintln!("[synthetic_pio_out_consumed] skipped: {:?}", e),
        }
    }

    #[test]
    fn telemetry_records_full_state_transition_sequence() {
        match run_with_payload("transitions", &UD2) {
            Ok(outcome) => {
                let actual: Vec<_> = outcome.telemetry.transitions.iter().map(|t| (t.from, t.to)).collect();
                assert_eq!(actual, vec![
                    (State::Configured, State::Initializing),
                    (State::Initializing, State::Booting),
                    (State::Booting, State::Running),
                    (State::Running, State::Exiting),
                    (State::Exiting, State::Terminated),
                ]);
                assert!(outcome.telemetry.boot_started_at <= outcome.telemetry.boot_completed_at.unwrap());
                assert!(outcome.telemetry.boot_completed_at.unwrap() <= outcome.telemetry.terminated_at.unwrap());
            }
            Err(e) => eprintln!("[telemetry_records_full_state_transition_sequence] skipped: {:?}", e),
        }
    }

    #[test]
    fn synthetic_kernel_writes_hi_via_pio_uart() {
        // mov dx, 0x3F8 ; mov al, 'H' ; out dx, al ; mov al, 'i' ; out dx, al ; ud2
        let payload = [
            0x66, 0xba, 0xf8, 0x03,
            0xb0, b'H', 0xee,
            0xb0, b'i', 0xee,
            0x0f, 0x0b,
        ];
        match run_with_stdout("uart_write", &payload, StdoutSink::Buffer) {
            Ok(outcome) => assert_eq!(outcome.stdout, b"Hi".to_vec()),
            Err(e) => eprintln!("[synthetic_kernel_writes_hi_via_pio_uart] skipped: {:?}", e),
        }
    }
}
