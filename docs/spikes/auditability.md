# Spike: auditability

Tests architecture.md §12.3: each subsystem's responsibility, knowledge, and failure modes can be described in a paragraph; cross-subsystem reach-throughs are absent.

## Method

Write a paragraph for each of the five subsystems covering (i) what it owns, (ii) what it knows about, (iii) what it deliberately does not know, (iv) the failure modes that originate there. Then walk the three refutation conditions in §12.3 and check each.

## Subsystem paragraphs

### `kvm`

Owns the host's interaction with `/dev/kvm`. Opens the device, creates a VM, creates vCPUs, registers guest memory regions, sets initial vCPU register state, runs vCPUs and surfaces their exit reasons. Knows the KVM ABI version it requires, the x86_64 vCPU state layout, and the small set of `KVM_EXIT_*` reasons that mean the VM is fundamentally unusable. Does not know the Linux boot protocol, the shape of any virtio device, the lifecycle state machine, or how the configuration was parsed. Failures here are `KvmError` variants: ioctl errno, ABI mismatch, unhandled exit reason. Anything more semantic — "kernel panicked", "device queue malformed" — is by definition a different subsystem's concern.

### `boot`

Owns the Linux x86_64 boot protocol. Takes uncompressed `vmlinux` bytes plus optional initramfs bytes, writes them into a `GuestMem` reference at the documented addresses, builds `boot_params`, builds identity-mapped page tables, and returns a `RegState` describing the initial vCPU registers per the protocol. Knows the protocol, the ELF format, x86_64 paging. Does not know KVM beyond producing the `RegState` value that `lifecycle` will apply via `kvm`; does not know virtio devices; does not run the vCPU. Failures here are `HostError` variants for malformed ELF, kernel that does not fit, unsupported features in the loaded image.

### `devices`

Owns virtio-block, virtio-net, virtio-vsock, and the 8250 UART. Each device implements the `MmioDevice` trait and is dispatched via `MmioBus` on MMIO exits. Knows the virtio 1.x specification, MMIO transport semantics, and the host-side I/O resources (open files, TAP fd, vsock listener fd) it was handed at construction. Does not know KVM internals beyond receiving MMIO exits, does not know the lifecycle, does not parse configuration. Failures here are `GuestError` variants: malformed descriptor chain, oversized field, descriptor chain cycle, MMIO offset out of the device's region, unexpected PIO. The default assumption is that the guest is hostile; bounds-check before dispatch and never panic on guest input.

### `lifecycle`

Owns the canonical state machine, configuration validation, the run-to-completion loop, error propagation, telemetry collection. Knows all other subsystems via their public interfaces and nothing else. Drives `kvm` (open, create VM, run vCPUs), `boot` (load kernel, prime vCPU), `devices` (construct, dispatch MMIO). Does not reach into any subsystem's internals; if it needs something, the boundary widens with explicit justification. Failures here are validation errors, illegal state transitions, and aggregated propagations of subsystem failures wrapped into `IngotError`.

### `jailer`

Owns process isolation: cgroups v2 setup, user/mount/PID/network namespace creation, seccomp filter installation, privilege drop, exec of the VMM binary. Knows Linux's process-isolation primitives and the seccomp filter contract. Treats the VMM as an opaque binary it isolates and execs; does not link `ingot` as a library, does not share types or memory with it. Failures here are `HostError` variants for cgroups setup, namespace creation, seccomp install, capability drop, and exec — each leaving the system in a known-not-launched state.

## Refutation checks

### 1. Does any paragraph reference types or files outside its directory?

- `kvm` references `KvmError` (its own), `KVM_EXIT_*` (the KVM ABI, not a sibling subsystem). No leak.
- `boot` references `GuestMem` (a top-level type, not a sibling subsystem) and `RegState` (its own contract type defined in `kvm/` per the architecture, but it is a *value* boot returns rather than internals it consults). The boundary is the type, not the implementation.
- `devices` references `MmioDevice`/`MmioBus` (its own), `GuestError` (its own).
- `lifecycle` references each subsystem by name — that is the single subsystem allowed to do so, by §3.1.4.
- `jailer` references `HostError` (a top-level type, the shared error domain) and process-isolation syscalls. No reach into ingot internals — explicitly the opposite.

**Pass.** No paragraph leaks across a boundary it should not cross.

### 2. Can the trust-boundary rules be stated without implementation detail?

From architecture.md §3.3 and §7:

- _Caller ↔ library:_ "Caller is trusted; library validates inputs and never panics on caller input." Stateable as a rule, refutable without code.
- _VMM ↔ guest:_ "Guest is hostile. Every byte of guest-controlled input is bounds-checked at the boundary it crosses." Stateable, refutable.
- _VMM ↔ host kernel:_ "The seccomp filter is the contract; the VMM issues exactly the syscalls it provably needs." Stateable, refutable.
- _Jailer ↔ host:_ "Elevated privileges held only between fork and execve; all isolation primitives in place before execve." Stateable, refutable.

**Pass.** Every rule is testable as a property without referring to a particular function or line.

### 3. Do the §3 interfaces describe a real codepath without leaking internal types?

Trace the boot codepath from `Vm::new` to the first guest instruction:

1. `Vm::new(config)` → `lifecycle::Runtime::new(config)`. Validates `Config`. Calls `Kvm::open()` → `KvmError`. Calls `VmFd::map_memory(...)` → `KvmError`. Constructs `GuestMem`, `MmioBus`, devices.
2. `Vm::run(self)` → `lifecycle::run(runtime)`. Calls `boot::load_kernel(image, &mut mem)` → `KernelImage` or `HostError`. Calls `boot::build_boot_params(...)` → `BootParams`. Calls `boot::prime_vcpu(...)` → `RegState` or `HostError`.
3. `lifecycle` applies `RegState` via `VcpuFd::set_regs`. Enters the run loop.
4. `VcpuExit::MmioWrite { addr, data, len }` → `MmioBus::write(addr, data)` → `GuestError` on out-of-range or malformed.

Every type that appears in this trace is named in §3 or is a sibling-public type re-exported by `lib.rs`. Nothing internal to a subsystem leaks. The trace is fully describable without opening any subsystem's source files.

**Pass.**

## Verdict

**Confirmed.** The Phase 0 architecture is auditable in the sense the constitution wants: a reader can understand each subsystem's reason for existing in a paragraph, the trust boundaries are property-style rules, and a real codepath traces through documented types only. The paragraphs above become the seed of the per-subsystem one-page docs called for in SPEC.md §5.4 (Phase 8 deliverable); they should be moved verbatim to `docs/<subsystem>.md` files when those files are created.
