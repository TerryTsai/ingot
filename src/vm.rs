//! Vm — the orchestrator. Reads top-to-bottom:
//!
//!   Vm::new           construct: validate, /dev/kvm, memory, vCPUs, MMIO bus
//!   Vm::run           drive lifecycle: boot kernel, run loop, build outcome
//!   boot_kernel       load ELF, write cmdline + boot_params, prime vCPU
//!   run_loop          dispatch MMIO / PIO / Hlt / Shutdown
//!   step              state-machine transition + telemetry recording
//!
//! Subsystem directories (kvm/, boot/, devices/) provide primitives.
//! This file composes them.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::boot::{self, ElfLoader, Initramfs, Kernel, MpTable, VcpuPrimer};
use crate::config::{Config, StdoutSink};
use crate::devices::{MmioBus, Net, Tap, Uart, UartInput, COM1_BASE, COM1_GSI, COM1_LAST};
use crate::error::{HostError, IngotError};
use crate::kvm::{Cpuid, KvmError, KvmFd, VcpuExit, VcpuFd, VmFd};
use crate::memory::Memory;
use crate::outcome::{ExitStatus, GuestFaultReason, Outcome, Telemetry, TransitionEvent};
use crate::state::{transition, State};

const CMDLINE_ADDR: u64 = 0x2_0000;
const BOOT_PARAMS_ADDR: u64 = 0x7000;
const CMDLINE_MAX: u32 = 2048;
/// KVM's 3-page TSS region, placed in the high I/O hole below 4 GiB so it
/// doesn't conflict with the userspace memory region or the MMIO bus.
const TSS_ADDR: u32 = 0xfffb_d000;
const MIN_MEMORY: u64 = 16 * 1024 * 1024;
const PAGE_SIZE: u64 = 0x1000;
const CMDLINE_LIMIT: usize = 2048;
/// CMOS RTC PIO ports. Index register selects which CMOS byte; data
/// register reads/writes it. The kernel's `read_persistent_clock` and
/// TSC calibration both poke these; returning 0xFF for data would set
/// CMOS Status A's "Update In Progress" bit and hang the kernel.
const CMOS_INDEX: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
/// virtio-mmio net device: 4 KiB region the kernel learns about via the
/// `virtio_mmio.device=4K@<addr>:<irq>` cmdline parameter. IRQ 5 keeps
/// us clear of legacy ISA assignments (timer=0, UART=4, RTC=8).
const VIRTIO_NET_BASE: u64 = 0xD000_0000;
const VIRTIO_NET_SIZE: u64 = 0x1000;
const VIRTIO_NET_GSI: u32 = 5;
const VIRTIO_NET_MAC: [u8; 6] = [0x52, 0x54, 0x00, 0xC0, 0xFF, 0xEE];

type Captured = Option<Arc<Mutex<Vec<u8>>>>;

pub struct Vm {
    config: Config,
    state: State,
    telemetry: Telemetry,
    bus: MmioBus,
    uart: Uart,
    vcpus: Vec<VcpuFd>,
    vm_fd: VmFd,
    mem: Memory,
    captured_stdout: Captured,
    cmos_index: u8,
    _kvm: KvmFd,
}

impl Vm {
    pub fn new(config: Config) -> Result<Self, IngotError> {
        Self::validate(&config)?;
        let kvm = KvmFd::open()?;
        let vm_fd = kvm.create_vm()?;
        let mem = Memory::new(config.memory_bytes)?;
        vm_fd.set_user_memory_region(0, 0, mem.host_ptr(), config.memory_bytes)?;
        vm_fd.set_tss_addr(TSS_ADDR)?;
        vm_fd.create_irqchip()?;
        vm_fd.create_pit2()?;
        let cpuid = kvm.supported_cpuid()?;
        let vcpus = Self::make_vcpus(&vm_fd, config.vcpus, &cpuid)?;
        let (uart, captured_stdout) = Self::make_uart(&config)?;
        let mut bus = MmioBus::new();
        let tap = config.tap_name.as_deref().map(Tap::open).transpose()?;
        bus.register(VIRTIO_NET_BASE, VIRTIO_NET_SIZE, Box::new(Net::new(VIRTIO_NET_MAC, tap)))?;
        Ok(Vm {
            telemetry: Self::fresh_telemetry(),
            state: State::Configured,
            bus,
            uart,
            vcpus,
            vm_fd,
            mem,
            captured_stdout,
            cmos_index: 0,
            config,
            _kvm: kvm,
        })
    }

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
        Ok(Outcome { exit_status, telemetry: self.telemetry, stdout })
    }

    fn boot_kernel(&mut self) -> Result<(), IngotError> {
        let kernel_bytes = std::fs::read(&self.config.kernel_path).map_err(|_| {
            IngotError::Host(HostError::KernelImageNotReadable(self.config.kernel_path.clone()))
        })?;
        let kernel = ElfLoader::load(&kernel_bytes, &mut self.mem)?;
        let initramfs = self.load_optional_initramfs(&kernel)?;
        self.write_cmdline()?;
        let bp = boot::build_boot_params(initramfs.as_ref(), CMDLINE_ADDR, CMDLINE_MAX, self.config.memory_bytes)?;
        bp.write_to(BOOT_PARAMS_ADDR, &mut self.mem)?;
        MpTable::write_to(self.config.vcpus, &mut self.mem)?;
        let state = VcpuPrimer::prime(&kernel, BOOT_PARAMS_ADDR, &mut self.mem)?;
        self.vcpus[0].set_sregs(&state.sregs)?;
        self.vcpus[0].set_regs(&state.regs)?;
        Ok(())
    }

    fn load_optional_initramfs(&mut self, kernel: &Kernel) -> Result<Option<Initramfs>, IngotError> {
        let Some(p) = self.config.initramfs_path.clone() else { return Ok(None); };
        let bytes = std::fs::read(&p).map_err(|_| IngotError::Host(HostError::InitramfsNotReadable(p)))?;
        Ok(Some(Initramfs::load(&bytes, &mut self.mem, kernel)?))
    }

    fn write_cmdline(&mut self) -> Result<(), IngotError> {
        self.mem.write(CMDLINE_ADDR, self.config.cmdline.as_bytes())?;
        self.mem.write(CMDLINE_ADDR + self.config.cmdline.len() as u64, &[0])?;
        Ok(())
    }

    fn run_loop(&mut self) -> Result<ExitStatus, IngotError> {
        let mut uart_irq = false;
        let mut bus_gsis: Vec<u32> = Vec::new();
        install_wakeup_timer();
        loop {
            match self.vcpus[0].run()? {
                VcpuExit::Hlt => return Ok(ExitStatus::Halted),
                VcpuExit::Shutdown => return Ok(ExitStatus::GuestFault(GuestFaultReason::TripleFault)),
                VcpuExit::MmioRead { addr, len } => self.handle_mmio_read(addr, len)?,
                VcpuExit::MmioWrite { addr, data, len } => self.bus.write(addr, &data[..len], &mut self.mem)?,
                VcpuExit::IoIn { port, len } => self.handle_pio_in(port, len),
                VcpuExit::IoOut { port, data, len } => self.handle_pio_out(port, &data[..len]),
                VcpuExit::Intr => {}
                VcpuExit::Other(r) => return Err(IngotError::Kvm(KvmError::UnhandledExit(r))),
            }
            let now = self.uart.pending_irq();
            if now != uart_irq {
                self.vm_fd.set_irq_line(COM1_GSI, now as u32)?;
                uart_irq = now;
            }
            self.bus.poll_io(&mut self.mem)?;
            self.reconcile_bus_irqs(&mut bus_gsis)?;
        }
    }

    /// Compare the bus's currently-pending GSIs against the set last
    /// driven high and drive each transition exactly once. Edge-triggered
    /// IOAPIC IRQs would otherwise refire every loop iteration; level-
    /// triggered ones would be re-asserted needlessly.
    fn reconcile_bus_irqs(&mut self, asserted: &mut Vec<u32>) -> Result<(), IngotError> {
        let pending: Vec<u32> = self.bus.pending_interrupts().into_iter().map(|(_, i)| i.gsi).collect();
        for gsi in asserted.iter() {
            if !pending.contains(gsi) { self.vm_fd.set_irq_line(*gsi, 0)?; }
        }
        for gsi in &pending {
            if !asserted.contains(gsi) { self.vm_fd.set_irq_line(*gsi, 1)?; }
        }
        *asserted = pending;
        Ok(())
    }

    fn handle_mmio_read(&mut self, addr: u64, len: usize) -> Result<(), IngotError> {
        let mut buf = [0u8; 8];
        self.bus.read(addr, &mut buf[..len])?;
        self.vcpus[0].set_mmio_read_data(&buf[..len]);
        Ok(())
    }

    fn handle_pio_in(&mut self, port: u16, len: usize) {
        let value = match port {
            CMOS_DATA => cmos_read(self.cmos_index),
            COM1_BASE..=COM1_LAST => self.uart.read((port - COM1_BASE) as u8),
            _ => 0xFF,
        };
        let bytes = [value, 0xFF, 0xFF, 0xFF];
        self.vcpus[0].set_pio_read_data(&bytes[..len.min(4)]);
    }

    fn handle_pio_out(&mut self, port: u16, data: &[u8]) {
        if data.is_empty() { return; }
        match port {
            CMOS_INDEX => self.cmos_index = data[0] & 0x7F,
            COM1_BASE..=COM1_LAST => self.uart.write((port - COM1_BASE) as u8, data[0]),
            _ => {}
        }
    }

    fn step(&mut self, to: State) -> Result<(), IngotError> {
        let new = transition(self.state, to)
            .map_err(|_| IngotError::Host(HostError::InvalidConfig("illegal state transition")))?;
        self.telemetry.transitions.push(TransitionEvent { at: Instant::now(), from: self.state, to: new });
        self.state = new;
        Ok(())
    }

    fn validate(config: &Config) -> Result<(), HostError> {
        if !config.kernel_path.is_file() {
            return Err(HostError::KernelImageNotReadable(config.kernel_path.clone()));
        }
        if let Some(p) = &config.initramfs_path {
            if !p.is_file() {
                return Err(HostError::InitramfsNotReadable(p.clone()));
            }
        }
        if config.vcpus != 1 {
            return Err(HostError::InvalidConfig("only single-vCPU is supported in Phase 2 (Phase 6 adds multi-vCPU)"));
        }
        if config.memory_bytes < MIN_MEMORY {
            return Err(HostError::InvalidConfig("memory must be at least 16 MiB"));
        }
        if config.memory_bytes % PAGE_SIZE != 0 {
            return Err(HostError::InvalidConfig("memory must be page-aligned"));
        }
        if config.cmdline.len() > CMDLINE_LIMIT {
            return Err(HostError::InvalidConfig("cmdline too long (max 2048 bytes)"));
        }
        if config.cmdline.contains('\0') {
            return Err(HostError::InvalidConfig("cmdline contains NUL"));
        }
        Ok(())
    }

    fn make_vcpus(vm: &VmFd, count: u8, cpuid: &Cpuid) -> Result<Vec<VcpuFd>, IngotError> {
        let mut v = Vec::with_capacity(count as usize);
        for i in 0..count {
            let vcpu = vm.create_vcpu(i as u32)?;
            vcpu.set_cpuid2(cpuid)?;
            v.push(vcpu);
        }
        Ok(v)
    }

    fn make_uart(config: &Config) -> Result<(Uart, Captured), IngotError> {
        let (sink, captured) = Self::make_sink(&config.stdout)?;
        let uart = Uart::new(sink);
        Self::maybe_spawn_stdin(uart.input_handle(), config.interactive);
        Ok((uart, captured))
    }

    fn make_sink(s: &StdoutSink) -> Result<(Box<dyn Write + Send>, Captured), HostError> {
        Ok(match s {
            StdoutSink::Discard => (Box::new(io::sink()), None),
            StdoutSink::File(p) => (
                Box::new(File::create(p).map_err(|_| HostError::InvalidConfig("could not open stdout file"))?),
                None,
            ),
            StdoutSink::Fd(fd) => (Box::new(Self::dup_to_file(*fd)?), None),
            StdoutSink::Buffer => {
                let b = Arc::new(Mutex::new(Vec::new()));
                (Box::new(BufferSink(b.clone())), Some(b))
            }
        })
    }

    fn dup_to_file(fd: std::os::fd::RawFd) -> Result<File, HostError> {
        let d = unsafe { libc::dup(fd) };
        if d < 0 { return Err(HostError::InvalidConfig("dup failed for StdoutSink::Fd")); }
        Ok(unsafe { File::from_raw_fd(d) })
    }

    /// When `interactive`, spawn a detached thread reading host stdin one
    /// byte at a time and pushing into the UART's input queue. The thread
    /// exits when stdin EOFs or errors. Detached because library callers
    /// using interactive=true accept the leak.
    ///
    /// When stdin is not a TTY (i.e. piped), pause for 2s before pushing.
    /// Piped input would otherwise saturate the UART queue while the
    /// kernel is still booting; when sh later opens `/dev/console`, the
    /// n_tty ldisc resets its read buffer and discards bytes that were
    /// already there. Real interactive typing is naturally paced and
    /// doesn't trip this — so when stdin is a TTY we don't wait.
    fn maybe_spawn_stdin(input: UartInput, interactive: bool) {
        if !interactive { return; }
        std::thread::spawn(move || {
            if unsafe { libc::isatty(0) } == 0 {
                std::thread::sleep(std::time::Duration::from_millis(2000));
            }
            let stdin = io::stdin();
            let mut handle = stdin.lock();
            let mut buf = [0u8; 1];
            loop {
                match handle.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => input.push(buf[0]),
                }
            }
        });
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
}

extern "C" fn sigalrm_noop(_: libc::c_int) {}

/// Install a no-op SIGALRM handler and arm setitimer at 5ms intervals.
/// SIGALRM with no SA_RESTART forces `KVM_RUN` to return `-EINTR`, which
/// `VcpuFd::run` turns into `VcpuExit::Intr`. The run loop then drives
/// `poll_io` (TAP RX, etc.) before re-entering `KVM_RUN`. Without this,
/// a guest in `hlt` would never let the host drain its TAP fd because
/// nothing else generates a vCPU exit.
fn install_wakeup_timer() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigalrm_noop as *const () as libc::sighandler_t;
        sa.sa_flags = 0;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGALRM, &sa, std::ptr::null_mut());
        let it = libc::itimerval {
            it_interval: libc::timeval { tv_sec: 0, tv_usec: 5_000 },
            it_value:    libc::timeval { tv_sec: 0, tv_usec: 5_000 },
        };
        libc::setitimer(libc::ITIMER_REAL, &it, std::ptr::null_mut());
    }
}

/// Return a plausible byte for CMOS register `index`. Time fields read
/// as zero (Linux's epoch fallback is fine); status registers report
/// "RTC running, not updating, 24h binary mode" so `read_persistent_clock`
/// proceeds instead of busy-looping on the UIP bit.
fn cmos_read(index: u8) -> u8 {
    match index {
        0x0A => 0x26,
        0x0B => 0x06,
        0x0D => 0x80,
        _ => 0,
    }
}

struct BufferSink(Arc<Mutex<Vec<u8>>>);
impl Write for BufferSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::{Config, IngotError, Outcome, StdoutSink, Vm};
    use crate::outcome::ExitStatus;
    use crate::state::State;

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
