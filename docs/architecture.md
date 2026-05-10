# Architecture

This document is the Phase 0 architecture deliverable required by [SPEC.md §4](../SPEC.md). It expands SPEC.md §3 with concrete interface shapes, memory-map decisions, and inter-subsystem contracts at the level of detail needed to be falsifiable.

SPEC.md is authoritative. Where this document conflicts with SPEC.md, SPEC.md wins and this document must be revised.

## 0. Status

Tentative. Every decision in this document is subject to revision based on the Phase 0 falsification spikes (cold boot, typed harness, auditability, embedded API ergonomics). Section 12 lists the specific claims each spike must validate.

## 1. System overview

```
                                 ┌──────────────────────────┐
                                 │       caller program     │
                                 └──────────────┬───────────┘
                                                │  control path: ingot library API
                                                │  data path:    AF_VSOCK fd from caller
                ┌───────────────────────────────┴────────────────────────────────┐
                │                          ingot (library)                       │
                │                                                                │
                │  ┌──────────┐    ┌──────────┐    ┌──────────────────────────┐  │
                │  │ lifecycle│◀──▶│   kvm    │    │         devices           │ │
                │  │  (state  │    │ (ioctl   │    │  uart  block  net  vsock  │ │
                │  │ machine) │    │  wrap)   │    │     all impl MmioDevice   │ │
                │  └────┬─────┘    └────┬─────┘    └──────────────┬─────────────┘│
                │       │               │                          │             │
                │       │       ┌──────────┐                       │             │
                │       └──────▶│   boot   │                       │             │
                │               │ (linux   │                       │             │
                │               │ protocol)│                       │             │
                │               └────┬─────┘                       │             │
                └────────────────────┼─────────────────────────────┼─────────────┘
                                     │  KVM_SET_REGS, vCPU run     │  MMIO exits
                                     ▼                              ▼
                                  ┌──────────────────────────────────────┐
                                  │         /dev/kvm  (host kernel)      │
                                  └──────────────────────────────────────┘

   ingot-jailer (separate binary) → cgroups + namespaces + seccomp → exec(ingot)
```

The caller links `ingot` as a library and drives the VM through the public API. The library composes five subsystems behind that API. The `ingot-jailer` binary is invoked separately to wrap any `ingot`-using process in isolation primitives before exec.

## 2. Subsystems

Each subsystem owns a directory under `src/`. No subsystem reaches across boundaries. Cross-subsystem communication happens via the interfaces in §3.

### 2.1 `kvm`

Owns: `src/kvm/`.

Exposes to `lifecycle`:
- `Kvm::open() -> Result<Kvm, KvmError>` — opens `/dev/kvm`, validates ABI version.
- `Kvm::create_vm() -> Result<VmFd, KvmError>` — creates a VM file descriptor.
- `VmFd::map_memory(slot, guest_phys, host_addr, size) -> Result<(), KvmError>` — registers a memory region.
- `VmFd::create_vcpu(id) -> Result<VcpuFd, KvmError>` — creates a vCPU.
- `VcpuFd::set_regs(regs)` / `set_sregs(sregs)` — initial register state.
- `VcpuFd::run() -> Result<VcpuExit, KvmError>` — runs until next exit.

Consumes: `libc`, `kvm-bindings`. No other ingot subsystems.

Out of scope: device emulation, boot protocol, configuration parsing. The only KVM exits this subsystem handles directly are those that mean the VM is unusable (`KVM_EXIT_FAIL_ENTRY`, `KVM_EXIT_INTERNAL_ERROR`); all other exits surface to `lifecycle`.

### 2.2 `boot`

Owns: `src/boot/`.

Exposes to `lifecycle`:
- `load_kernel(image: &[u8], guest_mem: &mut GuestMem) -> Result<KernelImage, HostError>` — parses uncompressed `vmlinux` ELF, copies segments into guest memory.
- `load_initramfs(image: &[u8], guest_mem: &mut GuestMem, kernel: &KernelImage) -> Result<InitramfsImage, HostError>` — places initramfs after kernel.
- `build_boot_params(kernel, initramfs, cmdline, mmio_bus_addr) -> BootParams` — constructs the `boot_params` struct.
- `prime_vcpu(vcpu, kernel: &KernelImage, params_addr: u64) -> Result<(), HostError>` — sets up identity-mapped page tables in guest memory and initial vCPU registers per the Linux x86_64 64-bit boot protocol.

Consumes: nothing. The boot subsystem is pure: it takes byte slices and writes into a `GuestMem` reference, and produces register/state values that `lifecycle` then applies via `kvm`.

Out of scope: MMIO devices, lifecycle orchestration, anything past the first guest instruction.

### 2.3 `devices`

Owns: `src/devices/`.

Exposes to `lifecycle`:
- `trait MmioDevice` — see §3.4 for the trait shape.
- Constructors: `Uart::new(stdout_sink)`, `Block::new(file, readonly)`, `Net::new(tap_fd)`, `Vsock::new(host_listener_fd, guest_cid)`.
- `MmioBus` — maps `(addr, size)` ranges to device handles, dispatches reads/writes.

Consumes: nothing from other subsystems. Devices receive their host-side I/O resources (open files, fds) from `lifecycle` at construction.

Out of scope: KVM run loop, configuration parsing, lifecycle state.

### 2.4 `lifecycle`

Owns: `src/lifecycle/`, `src/lib.rs`, `src/bin/ingot.rs`.

Exposes to caller: the public library API (§4) and the CLI binary that wraps it.

Consumes: `kvm`, `boot`, `devices`. This is the only subsystem that knows about all others.

Owns the run loop: drives vCPUs, dispatches MMIO exits to the bus, handles state transitions, propagates errors, captures telemetry.

### 2.5 `jailer`

Owns: `ingot-jailer/` (separate crate; added as a workspace member in Phase 7).

Does not link `ingot` as a library. The jailer is an opaque-binary launcher: it sets up cgroups v2, creates namespaces (user, mount, PID, net), installs the seccomp filter, drops privileges, then `execve`s the VMM binary. The seccomp filter is built from a static allowlist with the dangerous-syscall denylist as a defense-in-depth backstop.

Consumes: `libc`, `seccompiler` (or in-tree seccomp BPF builder — TBD by spike), nothing else.

## 3. Inter-subsystem interfaces

These are the load-bearing interfaces. They are defined here so each subsystem can be implemented independently against a stable contract.

### 3.1 Guest memory

```rust
pub struct GuestMem { /* opaque: a single mmap-backed region */ }

impl GuestMem {
    pub fn new(size_bytes: u64) -> Result<GuestMem, HostError>;
    pub fn write(&mut self, guest_phys: u64, data: &[u8]) -> Result<(), GuestError>;
    pub fn read(&self, guest_phys: u64, dst: &mut [u8]) -> Result<(), GuestError>;
    pub fn host_ptr(&self) -> *mut u8;
    pub fn size(&self) -> u64;
}
```

A single contiguous host mapping backs guest physical memory `[0, size)`. MMIO regions are *outside* this range and are not backed by `GuestMem`. Every read/write validates that `guest_phys + len <= size`; the validation is at this layer so subsystems above it cannot forget.

### 3.2 KVM exit dispatch

`VcpuFd::run()` returns a `VcpuExit` enum. The lifecycle loop pattern-matches:

```rust
match vcpu.run()? {
    VcpuExit::MmioRead { addr, len, data } => bus.read(addr, &mut data[..len])?,
    VcpuExit::MmioWrite { addr, data } => bus.write(addr, data)?,
    VcpuExit::Hlt => transition_to(Exiting),
    VcpuExit::Shutdown => transition_to(Exiting),
    VcpuExit::IoIn { .. } | VcpuExit::IoOut { .. } => Err(GuestError::UnexpectedPio),
    other => Err(KvmError::UnhandledExit(other)),
}
```

Port-I/O exits are explicitly rejected: this VMM is MMIO-only and the kernel is configured accordingly. A guest issuing PIO is a configuration error or an attack.

### 3.3 Boot → lifecycle handoff

`boot` does not run the vCPU. Its outputs are:
- A populated `GuestMem` (kernel image + initramfs + initial page tables + `boot_params`).
- A `RegState { rip, rsp, cr3, ... }` value describing the initial vCPU register state per the Linux 64-bit boot protocol.

`lifecycle` applies the register state via `kvm` and starts the run loop. This keeps `boot` free of any KVM dependency.

### 3.4 `MmioDevice` trait

```rust
pub(crate) trait MmioDevice: Send {
    /// Read `data.len()` bytes from `offset` within this device's MMIO region.
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError>;

    /// Write `data` at `offset` within this device's MMIO region.
    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError>;

    /// Pending interrupts, if any, for this device. Polled by lifecycle after each exit.
    fn pending_interrupt(&mut self) -> Option<Interrupt>;
}
```

Devices return `GuestError` for any guest-induced fault (out-of-range offset, malformed virtio descriptor, integer overflow in length fields). They do not panic. They do not log-and-ignore.

The `MmioBus` is an interval map keyed by `(base, size)` per device. Lookups are `O(log n)` over a small `n` (single-digit devices in the common case).

### 3.5 Jailer ↔ VMM contract

The jailer and VMM communicate through three channels and nothing else:

1. **Argv / environment:** the jailer passes the VMM's invocation as positional args after `--`.
2. **File descriptors:** any pre-opened resources (TAP fd, vsock listener fd, kernel image fd) are passed by number via `--fd-tap=N`, `--fd-vsock=N`, etc. The jailer allocates them; the VMM consumes them.
3. **Exit code:** the VMM's exit code is the jailer's exit code.

There is no shared memory, no IPC socket, no control channel. The jailer is fire-and-forget.

## 4. Public library API

The minimum-viable shape for Phase 6:

```rust
pub struct Config {
    pub kernel_path: PathBuf,
    pub initramfs_path: Option<PathBuf>,
    pub rootfs: Option<BlockSpec>,
    pub net: Option<NetSpec>,
    pub vsock: Option<VsockSpec>,    // caller provides host-side listener fd
    pub vcpus: u8,
    pub memory_bytes: u64,
    pub cmdline: String,
    pub stdout: StdoutSink,           // file, pipe, or in-memory buffer
}

pub struct Vm { /* opaque */ }

pub struct Outcome {
    pub exit_status: ExitStatus,      // Halted, KernelPanic, ResourceExhausted, ...
    pub telemetry: Telemetry,
    pub stdout: Vec<u8>,              // if StdoutSink::Buffer was used
}

impl Vm {
    pub fn new(config: Config) -> Result<Vm, IngotError>;
    pub fn run(self) -> Result<Outcome, IngotError>;
}
```

The common-case caller is three lines:

```rust
let cfg = Config { kernel_path, vcpus: 1, memory_bytes: 128 << 20, ..Config::default() };
let outcome = Vm::new(cfg)?.run()?;
println!("guest said: {}", String::from_utf8_lossy(&outcome.stdout));
```

The vsock data path is opt-in via `Config::vsock`. The caller is responsible for creating a host-side `AF_VSOCK` listener and passing its fd; the VMM wires it into the virtio-vsock device. The VMM never proxies bytes.

## 5. Guest physical memory map

```
0x0000_0000_0000  ┬──────────────────────┐
                  │  legacy / BIOS area  │  <1 MiB, mostly zero
0x0000_0010_0000  ├──────────────────────┤
                  │  kernel image        │  loaded from vmlinux ELF segments
                  ├──────────────────────┤
                  │  initramfs           │  immediately after kernel
                  ├──────────────────────┤
                  │  boot_params, gdt,   │
                  │  initial pagetables  │
                  ├──────────────────────┤
                  │  guest RAM           │  remainder up to memory_bytes
memory_bytes      ├──────────────────────┤
                  │  ── unmapped gap ──  │
0x0000_D000_0000  ├──────────────────────┤
                  │  MMIO bus            │  one region per device
0x0000_D000_F000  └──────────────────────┘
```

Specific addresses (subject to validation in the cold-boot spike):
- Kernel load address: `0x0010_0000` (1 MiB) — traditional bzImage load address.
- Initramfs: page-aligned address immediately past the kernel image.
- `boot_params`: `0x0000_7000`.
- Initial 4-level page tables: `0x0000_9000` (PML4) through `0x0000_C000`.
- MMIO bus base: `0xD000_0000`. Per-device size is 4 KiB. This sits below 4 GiB to avoid PAE complexity in the early kernel.

The host mapping for guest memory is a single `mmap(MAP_PRIVATE | MAP_ANONYMOUS)` of `memory_bytes`. MMIO is *not* host-mapped: MMIO accesses exit the guest and are dispatched to devices in the run loop.

## 6. Lifecycle state machine — transitions

For each transition: the trigger, the owner, what runs, and what failures map to which terminal state.

| From → To | Trigger | Owner | Operations | Failure → |
|-----------|---------|-------|------------|-----------|
| `Configured → Initializing` | `Vm::new()` returns | lifecycle | `GuestMem` allocate, `Kvm::open`, `create_vm`, register memory slots, construct devices, register MMIO bus, create vCPUs | `Failed` |
| `Initializing → Booting` | `Vm::run()` invoked | lifecycle | `boot::load_kernel`, `boot::load_initramfs`, `boot::build_boot_params`, `boot::prime_vcpu`, apply register state via `kvm` | `Failed` |
| `Booting → Running` | First vCPU `run()` returns without immediate fatal exit | lifecycle | record boot timestamp; enter run loop | `Failed` |
| `Running → Exiting` | `KVM_EXIT_HLT`, `KVM_EXIT_SHUTDOWN`, or guest-triggered fatal exit | lifecycle | record exit cause, drain pending device I/O, stop vCPUs | `Failed` if drain fails |
| `Exiting → Terminated` | All resources released | lifecycle | unmap memory, close fds, finalize telemetry | `Failed` if any release errors |
| _any → `Failed`_ | Any unrecoverable error | lifecycle | best-effort release of held resources, finalize telemetry with failure cause | terminal |

`Terminated` and `Failed` are both terminal but distinguishable in `Outcome`. `Failed` is not a way to flag "guest crashed"; a guest kernel panic is `Terminated` with `ExitStatus::KernelPanic`. `Failed` means *the VMM* could not function (e.g., KVM ioctl returned `EINVAL` mid-run).

The state machine is implemented as a Rust enum with the state value owned by the lifecycle subsystem. Transitions go through a single `transition(from, to)` function that asserts the legality of the move and emits a telemetry event. There are no booleans tracking sub-state.

## 7. Trust boundaries — concrete validation rules

### 7.1 Caller ↔ library

`Config` validation occurs entirely during `Vm::new`. Rules:
- All file paths exist and are readable.
- `vcpus >= 1` and `vcpus <= 32` (32 is arbitrary but bounded; expanded only with justification).
- `memory_bytes` is page-aligned, `>= 16 MiB`, `<= host RAM / 2`.
- `cmdline` is `<= 2048` bytes and contains no NUL.
- `BlockSpec::path` resolves to a regular file (no devices, no fifos, no symlinks to outside the cwd).

Validation failures return typed `HostError`. No panics on caller error.

### 7.2 VMM ↔ guest

Every byte of guest-controlled input is bounds-checked at the boundary it crosses:
- **Virtio descriptor chains:** descriptor indices `< queue_size`; descriptor lengths fit in remaining queue space; `addr + len` validated against `GuestMem::size()`; chain length capped at `queue_size` to prevent loops.
- **MMIO offsets:** dispatched to device only after `MmioBus` confirms `(addr, len)` falls in a single device's region.
- **Vsock packets:** packet length validated against guest-declared length and remaining queue space; src/dst CIDs ignored (the host listener is what matters).
- **Boot images:** ELF parsed by `boot::load_kernel`; segment offsets and sizes bounds-checked against the input slice and against `GuestMem` size before any copy.

Guest-induced violations return `GuestError`. They are recorded in telemetry and trigger transition to `Exiting` with `ExitStatus::GuestFault(reason)`. They never panic.

### 7.3 VMM ↔ host kernel

The seccomp filter is the only contract. Phase 7 builds it. Until then, the VMM runs with the inherited filter (none). The filter is built from an allowlist; adding a syscall requires a `docs/decisions/` entry naming the call, the codepath, and why it cannot be avoided.

### 7.4 Jailer ↔ host

The jailer holds elevated privileges only between fork and `execve`. Invariant: by the time `execve(ingot, ...)` runs, all of the following are true: cgroups attached, namespaces unshared, root mount pivoted, capabilities dropped to the empty set, seccomp filter loaded with `SECCOMP_FILTER_FLAG_TSYNC`, `no_new_privs` set.

## 8. State management — concrete types

```rust
pub struct Config { /* §4; immutable after Vm::new */ }

pub(crate) struct Runtime {
    state: State,
    mem: GuestMem,
    vm_fd: VmFd,
    vcpus: Vec<VcpuFd>,
    bus: MmioBus,
}

pub struct Telemetry {
    boot_started_at: Instant,
    boot_completed_at: Option<Instant>,
    terminated_at: Option<Instant>,
    transitions: Vec<TransitionEvent>,
    exit_status: Option<ExitStatus>,
    peak_rss_kb: Option<u64>,
}
```

`Config` is held by `Vm` for the lifetime of the VM, never mutated. `Runtime` is owned by `Vm` and not exposed across the API boundary. `Telemetry` is append-only during execution and exposed via `Outcome` after termination.

Functions take exactly the state they need:
- `boot::load_kernel(image, &mut GuestMem)` — no `Runtime`, no `Config`.
- `lifecycle::run_loop(&mut Runtime, &mut Telemetry)` — no `Config`.
- `Outcome::from(Telemetry, Vec<u8>)` — no `Runtime`.

## 9. Error model

```rust
pub enum IngotError {
    Kvm(KvmError),
    Guest(GuestError),
    Host(HostError),
}

pub enum KvmError { /* ioctl failures, ABI version mismatch, vCPU exit shapes */ }
pub enum GuestError { /* virtio violations, MMIO out-of-range, kernel panic */ }
pub enum HostError { /* file not found, mmap failed, TAP create failed */ }
```

Wrapping rules:
- Each domain's enum carries its own variants with source detail (errno, ioctl name, address, etc.).
- `IngotError` preserves the domain — `From<KvmError> for IngotError` is the only construction.
- No `Box<dyn Error>`. No string-only error variants. Every failure carries machine-actionable detail.
- Errors are returned, never logged-and-swallowed. Logging is *additional* to returning, never a substitute.

## 10. Data path versus control path

The library exposes the host-side vsock listener fd to the caller via `Config::vsock` (in) and never re-exposes it. Once `Vm::new` consumes the fd, the caller's reference to it is the same fd it passed in — the kernel handles connection establishment, byte transfer, flow control. The VMM has no `send_to_guest()`, no `read_from_guest()`, no buffer it owns on the data path.

Consequence: `ingot` cannot be MITM'd between caller and guest at the application layer. Whatever protocol the caller and guest agree on travels through `AF_VSOCK` directly.

## 11. File and module layout

Phase 0 sketches the structure below. Files marked _later_ are placeholders; they are added in their phase. Telemetry types live alongside `Outcome` (top-level) because they are a public-API concern, not a lifecycle internal — `lifecycle` writes them; callers read them.

```
src/
  lib.rs                — public surface: Vm, module declarations, re-exports
  config.rs             — Config, BlockSpec, NetSpec, VsockSpec, StdoutSink
  error.rs              — IngotError, HostError, From impls
  outcome.rs            — Outcome, ExitStatus, GuestFaultReason, Telemetry, TransitionEvent
  guest_mem.rs          — GuestMem
  kvm/
    mod.rs              — Kvm, VmFd, VcpuFd, VcpuExit, RegState, KvmError
    bindings.rs         — minimal raw ioctl wrappers (later)
  boot/
    mod.rs              — KernelImage, InitramfsImage, BootParams, load/build/prime
    elf.rs              — ELF parsing (later)
    pagetables.rs       — identity-map page table construction (later)
    boot_params.rs      — Linux x86_64 boot_params struct (later)
  devices/
    mod.rs              — MmioDevice trait, MmioBus, GuestError, Interrupt
    uart.rs             — 8250 UART
    block.rs            — virtio-block
    net.rs              — virtio-net
    vsock.rs            — virtio-vsock
    virtqueue.rs        — shared virtio queue parsing, validated (later)
  lifecycle/
    mod.rs              — Runtime, run loop entry point
    state.rs            — State enum + transition function
  bin/
    ingot.rs            — CLI: thin wrapper over public API
```

Phase 7 introduces `ingot-jailer` as a sibling crate. At that point the repository becomes a Cargo workspace with `ingot/` and `ingot-jailer/` as members. The jailer does not link `ingot`; it treats the VMM as an opaque binary it isolates and execs.

Every file in this layout is subject to the 100-line limit (SPEC.md §5.1). Subsystem directories will grow more files; existing files do not grow past the limit.

## 12. Falsification targets

The Phase 0 spikes test specific claims of this architecture. Each claim below has at least one spike that can refute it.

### 12.1 Cold-boot spike → §5 (memory map) and §3.3 (boot handoff)

Claim: this layout supports sub-50ms wall-clock from `Vm::run()` to first guest userspace instruction with a minimal kernel + init.

Refutation conditions:
- The boot protocol setup itself takes longer than ~5ms even on a minimized path.
- The chosen MMIO base or initramfs placement forces the kernel to do work that pushes past the budget.
- ELF parsing of an uncompressed `vmlinux` is slower than expected and `bzImage` direct-load needs to be reconsidered.

Output: either confirmation of the layout, or a list of specific changes (move boot_params, switch image format, change MMIO base).

### 12.2 Typed-harness spike → §4 (public API) and §10 (data path)

Claim: a typed task harness (manifest-producing, dry-run-capable) sits naturally on top of `Vm::new`/`Vm::run` plus a caller-owned vsock listener.

Refutation conditions:
- The harness needs the VMM to proxy bytes (would force violating §10).
- The harness needs a hook between subsystems that the public API does not expose.
- `Outcome` is missing fields (e.g., per-syscall audit log) that the harness cannot do without.

Output: either confirmation, or a list of additions to `Config`/`Outcome` and any new public types.

### 12.3 Auditability spike → §2 (subsystems) and §7 (trust boundaries)

Claim: each subsystem's responsibility, knowledge, and failure modes can be described in a paragraph; cross-subsystem reach-throughs are absent.

Refutation conditions:
- Any subsystem's paragraph references types or files outside its directory.
- Any trust-boundary rule cannot be stated without referring to implementation detail.
- The interfaces in §3 cannot describe a real codepath without leaking internal types.

Output: either confirmation, or a list of boundary violations the implementation must avoid.

### 12.4 Embedded API ergonomics spike → §4 (public API)

Claim: rewriting a small real program (a command-runner, a code-evaluator) on top of this API takes single-digit lines for the common case and surfaces no surprise frictions.

Refutation conditions:
- The minimal example is more than ~10 lines for a trivial use case.
- A common need (capturing stdout, setting a timeout, getting exit status) is awkward.
- `Config` is missing common knobs or has them in awkward positions.

Output: either confirmation, or a list of API changes — renames, defaults, new methods.

## 13. Open architectural questions

These are deliberately not yet decided. Each must be resolved before the relevant phase begins.

- **Threading model.** Phase 1 will run a single vCPU in a single thread. Multi-vCPU support requires deciding whether each vCPU gets its own OS thread (simpler, more memory) or all vCPUs share an event loop (harder, more cache-friendly). Defer to Phase 6.
- **Stdout capture.** `StdoutSink` shape is sketched but not fixed. Phase 2's UART implementation will inform the final choice.
- **Initramfs vs rootfs precedence.** If both are configured, do we treat initramfs as the boot environment and rootfs as a mount inside the guest, or refuse the combination? Resolve in Phase 3.
- **Seccomp filter format.** Hand-written BPF, `libseccomp`-style builder, or in-tree DSL? Resolve in Phase 7 after observing the actual syscall set.
