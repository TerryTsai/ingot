# Ingot

A minimal KVM-based virtual machine monitor for short-lived, hardware-isolated compute

## 0. Document Purpose and Scope

This document is the canonical specification for the Ingot project. It defines what Ingot is, what it is not, the architectural foundations on which it must be built, the order in which it must be built, and the principles that govern decisions made during development.

This document is binding. Deviation from it requires explicit revision of this document first, not commits that contradict it.

The intended reader is an engineer implementing Ingot from scratch with no prior context on the project beyond this document. Where ambiguity exists in the specification, default to the principle of least scope and surface the ambiguity for resolution rather than choosing silently.

## 1. Project Identity

Name: Ingot

One-line description: Hardware-isolated short-lived compute.

Longer description: Ingot is a minimal Virtual Machine Monitor (VMM) built on KVM that creates and runs ephemeral Linux microVMs for single-task workloads. It is library-first, x86_64-only initially, and is designed as a substrate for executing untrusted or agent-generated code with hardware-level isolation.

License: Apache 2.0.

Language: Rust.

Naming convention rules:

- The name "Ingot" is the project's only metaphor. APIs, modules, subsystems, CLI verbs, and internal types do not extend the metallurgy theme.
- Subsystem names are plain functional descriptors (`kvm`, `boot`, `devices`, `lifecycle`, `jailer`).
- CLI verbs are plain English (`run`, `inspect`, `kill`), never metaphor-extended (`smelt`, `cast`, `forge`).
- Documentation tone is factual and unromantic. The name carries the evocation; nothing else needs to.

## 2. Constitution

### 2.1 What Ingot is

Ingot creates a Linux microVM, runs a single workload to completion inside it, captures the result, and tears the VM down. It is a library that other programs embed, with a thin CLI for one-shot use.

### 2.2 What Ingot is not

Ingot is not a container runtime. It is not a long-lived VM platform. It is not a managed service. It is not a competitor to Firecracker, QEMU, Cloud Hypervisor, or any other VMM. It is not a product. It is not feature-comparable to any of the above and will never be.

### 2.3 The five foundational commitments

These are binding. Decisions that violate any of them are wrong, even when they would be locally convenient.

1. **Scope discipline over feature breadth.** Every feature must pay rent. No feature is added without either a corresponding deletion or an explicit, written justification recorded in the project's design log. Scope creep is the project's primary failure mode and is treated as such.
2. **Foundations validated by spike, not by elegance.** No foundational design is considered settled until it has survived a one-off implementation attempt against each long-term ambition. Architecture that looks beautiful on paper but cannot support its goals is the worst possible outcome. Falsification spikes are a required step in the design phase, not optional.
3. **Hostile workloads by default.** Every workload running inside Ingot is treated as untrusted, including the project author's own workloads. The jailer, seccomp filter, egress proxy, and resource limits are non-negotiable. There are no "trusted mode" bypasses. The moment a bypass exists, the project's threat model collapses and so does its reason to exist.
4. **Library first, daemon never (until forced).** Ingot is embedded in caller programs as a library. A CLI exists for one-shot use, implemented as a thin shell over the library. An HTTP control plane is not built unless and until concrete, written requirements make it unavoidable. Most existing VMM API complexity comes from premature daemonization and the project will not repeat the mistake.
5. **Readable code is auditable code.** Primitive files do not exceed 100 lines of production code; the orchestrator (`vm.rs`) and inline test modules are exempt. No function exceeds 20 lines. Every primitive file follows the same template (see §5.1). Every abstraction justifies itself in writing. The code is the documentation; the documentation is the design doc; the design doc is this constitution. The auditability differentiator is purchased through these constraints, not through later cleanup.

### 2.4 The differentiator axes (private, not public)

Ingot aims, eventually, to be measurably better than alternatives on four axes:

- **Cold boot latency.** Target: workload-executing inside the guest in under 50ms, with a stretch goal of under 30ms.
- **Embedded API ergonomics.** A library API in Rust that makes embedding Ingot in a larger program a single-digit line count for the common case.
- **Agent-shaped harness support.** Library primitives that make it natural to build typed, manifest-producing, dry-run-capable task harnesses on top of Ingot.
- **Auditability.** Small codebase, documented threat model, published fuzz coverage, eventual third-party audit.

These are private aims. They are not advertised, not promised, not benchmarked publicly until they are real. The project is presented externally as a pedagogical reference implementation. Differentiators emerge as observed properties, not as marketing.

### 2.5 Out of scope, permanently

The following will never be added to Ingot. Requests to add them will be closed without implementation.

- Windows, BSD, or any non-Linux guest OS.
- GPU passthrough, USB, sound, PCI passthrough, or any "rich" device emulation.
- PCI transport for virtio (MMIO transport only).
- Live migration.
- Multi-tenant density optimizations.
- Long-lived VM lifecycle features: memory hot-plug, ballooning, NUMA awareness.
- A managed-service offering or hosted product.
- Microvm metadata service (MMDS) or equivalent guest-facing config service.
- CPU templates or feature-masking for cross-host migration.

### 2.6 Out of scope, deferred

The following are not built in the initial implementation. Adding them requires a written RFC that justifies the addition against this constitution and updates this document.

- aarch64 support.
- Snapshot and restore.
- Multi-VM-per-process orchestration.
- A daemon control plane with HTTP or gRPC API.
- virtio-fs.
- io_uring or other async I/O backends for block devices.

### 2.7 Anti-goals

Written explicitly so they are harder to drift into:

- "Be Firecracker but better."
- "Be the standard for self-hosted agent sandboxes."
- "Match feature parity with [any other VMM]."
- "Be production-ready for arbitrary users."
- Adding a feature because someone asked.
- Adding a feature because it would be cool.
- Adding a feature because it would be easy.
- Optimizing on any axis other than the four chosen differentiators.
- Documenting features that don't yet exist.
- Premature abstraction in pursuit of imagined future flexibility.

## 3. Architecture

### 3.1 Subsystem boundaries

Ingot is composed of five subsystems. Every line of code lives in exactly one of them. Subsystems communicate only through documented interfaces.

#### 3.1.1 `kvm` — KVM ioctl wrapper

Responsibilities: opening `/dev/kvm`, creating VMs, creating vCPUs, mapping guest memory regions, running vCPUs, handling `KVM_EXIT` reasons that must be addressed at the KVM layer.

Knows about: KVM ABI, x86_64 vCPU state, guest physical memory layout.

Does not know about: Linux boot protocol, virtio devices, lifecycle, configuration.

#### 3.1.2 `boot` — Linux boot protocol

Responsibilities: parsing uncompressed `vmlinux` ELF images, constructing `boot_params`, building initial identity-mapped page tables, setting initial vCPU register state, placing the kernel and initramfs in guest memory at the correct addresses.

Knows about: Linux x86_64 boot protocol, ELF format, x86_64 paging.

Does not know about: virtio devices, lifecycle orchestration, the KVM run loop beyond initial register setup.

#### 3.1.3 `devices` — virtio device implementations

Responsibilities: virtio-block, virtio-net, virtio-vsock (all virtio devices use MMIO transport — no PCI), plus the legacy 8250 UART. The 8250 is exposed at PIO COM1 (0x3F8) because Linux's 8250 driver auto-detects COM1 with no DT/ACPI plumbing; this is a one-time concession to legacy x86 that costs ~100 lines and unblocks any stock kernel. A minimal CMOS RTC shim (PIO 0x70/0x71, read-only "not updating, time = 0") lives inline in `vm.rs` rather than as a device, because the kernel's `read_persistent_clock` only needs UIP=0 to proceed and the shim is two register reads — adding a device type for it would be premature abstraction. virtio devices implement a uniform internal `MmioDevice` trait.

Knows about: virtio 1.x specification, MMIO transport semantics, the 8250 register layout, host-side I/O backing each device.

Does not know about: KVM internals beyond MMIO/PIO exit handling, lifecycle, configuration parsing.

#### 3.1.4 `lifecycle` — VM lifecycle state machine

Responsibilities: parsing and validating configuration, instantiating subsystems in the correct order, the run-to-completion loop, teardown, error propagation, telemetry collection.

Knows about: all other subsystems, the canonical state machine, configuration semantics.

Does not know about: implementation details of any subsystem beyond their public interfaces.

#### 3.1.5 `jailer` — separate binary

Responsibilities: cgroups v2 setup, namespace creation (user, mount, PID, network), seccomp filter installation, resource limit enforcement, privilege drop, exec of the VMM binary.

Knows about: Linux process isolation primitives, the seccomp filter contract.

Does not know about: VMM internals. The jailer treats the VMM as an opaque binary it isolates and execs.

### 3.2 The lifecycle state machine

A VM exists in exactly one of the following states. Transitions are explicit and atomic.

```
Configured  ─→  Initializing  ─→  Booting  ─→  Running  ─→  Exiting  ─→  Terminated
                     │                │           │           │
                     └────────────────┴───────────┴───────────┴────→  Failed
```

- **Configured**: Configuration parsed and validated. No resources allocated.
- **Initializing**: Memory mapped, devices constructed, vCPUs created. KVM resources held.
- **Booting**: Kernel loaded, vCPUs started, awaiting first guest instruction execution.
- **Running**: Guest is executing. The expected steady state during workload execution.
- **Exiting**: Guest has signaled shutdown or hit a terminal condition. Teardown in progress.
- **Terminated**: All resources released. Telemetry available. Terminal state.
- **Failed**: Unrecoverable error. Resources released best-effort. Terminal state. Distinct from Terminated.

Every transition is logged. The state machine is exhaustive: there are no implicit states, no booleans tracking sub-state, no conditional logic that depends on undocumented state. If a needed state isn't in this list, the list is wrong and must be updated before code is written that needs it.

### 3.3 Trust boundaries

Four boundaries, each with a documented contract.

#### 3.3.1 Caller ↔ library

Caller is trusted. Library validates inputs (paths exist, sizes are sane, configuration is internally consistent) but does not assume adversarial behavior from the caller. Errors are typed and surfaced cleanly. Panics are bugs.

#### 3.3.2 VMM ↔ guest

Guest is hostile. Every byte read from guest memory is validated. Every device implementation assumes adversarial descriptor chains, malformed virtio queue state, malicious vsock payloads, attempts to trigger integer overflow in size fields, attempts to point descriptors outside guest memory, attempts to construct rings that loop forever. The default assumption is that the guest is trying to escape, and the code is structured to make escape attempts visible rather than silent.

#### 3.3.3 VMM ↔ host kernel

Host is trusted, but the VMM process is itself sandboxed. The seccomp filter installed by the jailer is the contract: the VMM is permitted exactly the syscalls it provably needs, and no others. Adding a syscall to the filter requires written justification.

#### 3.3.4 Jailer ↔ host

The jailer runs with elevated privileges briefly to set up isolation primitives, then drops privileges before exec'ing the VMM binary. The privilege window is the smallest possible. No long-lived elevated process exists.

### 3.4 State management

State is partitioned into three categories. Code that mixes them is wrong and must be refactored.

**Config** (immutable after VM creation): kernel path, rootfs path, vCPU count, memory size, network specification, resource limits, seccomp filter selection, vsock CID. Set once, read many times, never mutated after the Configured → Initializing transition.

**Runtime state** (mutable during execution): vCPU exit reasons, device queue positions, guest memory dirty tracking (if added later), current resource usage. Owned by the lifecycle subsystem; mutations happen in well-defined points in the state machine, not opportunistically.

**Telemetry** (write-only by VMM, readable externally): boot timing, exit code, captured stdout/stderr, peak resource usage, audit log entries. Append-only during VM lifetime. Read after Terminated or Failed for inspection.

Each category has its own type in the codebase. Functions that operate on Config do not take Runtime state. Functions that emit Telemetry do not read Runtime state directly; they receive it via the lifecycle subsystem.

### 3.5 Error model

Three error domains, each with its own type. The library API surfaces a unified `IngotError` that wraps these without losing source-domain information.

- `KvmError`: KVM ioctl failures, hardware feature mismatches, KVM ABI incompatibilities.
- `GuestError`: guest-caused failures — malformed virtio queues, kernel panic in guest, OOM in guest, guest-triggered MMIO into invalid ranges.
- `HostError`: host-side failures — file not found, TAP device creation failed, seccomp filter installation failed, cgroups setup failed.

Errors are never silently swallowed. Errors are never converted to logs without being also returned. Panics are bugs and trigger CI failure.

### 3.6 Data path versus control path

The caller drives the VM via the library API (control path). The caller communicates with the guest workload via vsock (data path). The two channels never cross.

Specifically: the library API does not provide methods like `send_to_guest(bytes)` or `read_from_guest()` that proxy data through the VMM. The VMM sets up the vsock channel during initialization and exposes the host-side socket to the caller; from there, caller-to-guest communication is the caller's responsibility, mediated by the kernel's vsock implementation.

This separation is non-negotiable. It is the largest single contributor to keeping the VMM small, the API simple, and the audit story clean.

## 4. Implementation Phases

Each phase has a clear "done" criterion and produces something runnable end-to-end before the next begins. No phase begins until the previous phase's done criterion is met.

### Phase 0 — Design and falsification (3 weeks minimum, longer if needed)

Deliverables:

- This document, finalized.
- Architecture document with subsystem boundaries, trust boundaries, state model, and error model fully specified.
- Library API sketched as Rust trait/struct definitions, no implementation.
- Falsification spike for cold boot: paper analysis or throwaway code demonstrating that the proposed architecture can support sub-50ms boot.
- Falsification spike for typed harness: a 200-line sketch of a harness on top of the proposed API, demonstrating it feels right.
- Falsification spike for auditability: written paragraphs describing each subsystem's responsibility, with crisp boundaries.
- Falsification spike for embedded API ergonomics: example caller code in a hypothetical user program.

Done criterion: The architecture survives all four falsification attempts, or has been revised to. No code is written for production until this phase completes.

### Phase 1 — KVM hello world (1 week)

Deliverables:

- The `kvm` subsystem in isolation.
- A test binary that opens `/dev/kvm`, creates a VM, creates a vCPU, runs 4KB of real-mode guest code, and observes a `hlt` exit.
- Tests exercising every public API function in the `kvm` subsystem.

Done criterion: ~150 lines of Rust, zero dependencies beyond `libc` and `kvm-bindings`, all tests pass.

### Phase 2 — Linux boot to serial console (3 weeks)

Deliverables:

- The `boot` subsystem.
- 8250 UART device implementation in `devices`.
- A test that direct-loads an uncompressed `vmlinux`, sets up `boot_params`, identity-maps initial page tables, jumps to kernel entry, and produces visible output on the serial console.
- A minimal initramfs (busybox + a shell) that drops to a prompt over serial.

Done criterion: `cargo run --bin ingot -- run --kernel vmlinux --initramfs init.cpio` boots to an interactive shell over serial within reasonable time.

### Phase 3 — virtio-block (2 weeks)

Deliverables:

- virtio-block device implementation in `devices`.
- virtio 1.x compliance, MMIO transport, file-backed.
- Property-based tests for virtio queue handling: malformed descriptor chains, oversized descriptors, looping rings, indirect-descriptor edge cases.

Done criterion: VM boots from a virtio-block rootfs (no initramfs required) and reads/writes correctly.

### Phase 4 — virtio-net (2 weeks)

Deliverables:

- virtio-net device implementation in `devices`.
- TAP device creation on host, virtio queues for rx/tx.
- No networking policy in the VMM beyond TAP setup.

Done criterion: VM can ping an external address and resolve DNS through a host-side bridge configured externally.

### Phase 5 — virtio-vsock (2 weeks)

Deliverables:

- virtio-vsock device implementation in `devices`.
- Host-side library function for sending data into the guest and receiving responses without using the network stack.

Done criterion: A test program calls a library function, the guest receives the message via `AF_VSOCK`, responds, and the host library function returns the response.

### Phase 6 — Library API and lifecycle (3 weeks)

Deliverables:

- The `lifecycle` subsystem implementing the state machine in section 3.2.
- Public library API: `ingot::Vm::new(config)?.run()?` works for the common case.
- CLI binary as a 50-line wrapper over the library.
- Configuration parsing with full validation.

Done criterion: A caller can embed Ingot in a Rust program and run a VM with three lines of code. The CLI runs the same workload from the command line. All errors are typed and surfaced through `IngotError`.

### Phase 7 — Jailer (3 weeks)

Deliverables:

- Separate `ingot-jailer` binary.
- cgroups v2 setup with configurable resource limits.
- User, mount, PID, and network namespace creation.
- Seccomp filter built from the observed syscall set, with denylist for known-dangerous syscalls.
- Privilege drop before VMM exec.
- Tests that demonstrate the VMM cannot perform out-of-scope operations.

Done criterion: The VMM running under the jailer cannot read files outside its declared mounts, cannot make network connections outside its declared interfaces, cannot exceed its resource limits, and cannot make syscalls outside its filter.

### Phase 8 — Integration hardening (4 weeks)

Deliverables:

- End-to-end CI that boots a real distro kernel on every commit.
- Property-based testing extended to all device implementations.
- A 24-hour churn test: create and destroy VMs in a loop, monitor host RSS, kernel slab, TAP/bridge cleanup, file descriptor leaks. Pass criterion: zero growth over the run.
- Fuzzing harness for every guest-controlled input surface (virtio queues, vsock payloads, MMIO writes).
- Documentation pass: every subsystem has a one-page design document; every public API has worked examples.

Done criterion: The project is ready for the author's own production use. Documentation is complete and accurate. The churn test passes.

### Phase 9 — Differentiator spikes (4 weeks)

Deliverables (each is a spike, not necessarily merged):

- Cold boot optimization spike: kernel config minimization, init replacement, direct-load tuning. Target: under 50ms wall-clock from `Vm::run()` to workload execution. Stretch: under 30ms.
- Typed harness spike: a separate library on top of Ingot demonstrating typed task ops, manifest production, dry-run support. Validates that the embedded API supports this shape cleanly.
- Audit-prep spike: written threat model, published fuzz coverage report, every critical path documented with adversarial scenarios.
- Embedded API ergonomics spike: rewrite a small real program (e.g., a command-runner, a code-evaluator) using Ingot. Time how long the rewrite takes. Note every API friction.

Done criterion: Each spike has produced either (a) confirmation that the foundation supports the goal, or (b) a written list of foundation flaws that Phase 10 must address.

### Phase 10 — Refactor based on what spikes revealed (3-6 weeks, contingent)

Deliverables:

- Foundation fixes for each flaw identified in Phase 9.
- Updated architecture document reflecting any structural changes.
- Re-run of the Phase 8 done criteria after refactor.

Done criterion: The foundation now demonstrably supports all four differentiator axes. The architecture document still describes the code accurately.

### Total timeline

Approximately 6-8 months of consistent focused work to reach the end of Phase 10. Phases 0-7 produce a working VMM; Phases 8-10 produce one suitable for the author's own use and capable of supporting the four differentiator axes.

## 5. Engineering Standards

### 5.1 Code style

- **One struct per file.** A primitive file owns exactly one struct and *every* method on it, plus any small helpers tightly bound to that struct (decoders, byte-packers, error-paths). Free functions that operate on the struct go in the same file. Splitting one primitive across multiple files (`foo.rs` + `foo_io.rs`, etc.) is forbidden — it scatters the implementation. When a file feels too long, the answer is to carve out a *new* primitive with its own struct in its own file, not to extract helpers into a sibling.
- **File length:** No hard cap. Primitive files are as long as they need to be. The orchestrator (`vm.rs`) is the longest by design.
- **Primitive file template (every primitive file structurally identical):**
  1. Module doc (`//!`).
  2. `use` imports.
  3. Module-private constants (grouped).
  4. `pub(crate) struct PrimitiveName { ... }` — file name is `primitive_name.rs` (snake_case of the struct).
  5. `impl PrimitiveName { ... }` — constructor first, then public methods, then private helpers.
  6. Optional trait impls (e.g. `impl MmioDevice for Foo`, `impl Drop`), in a fixed order.
  7. `#[cfg(test)] mod tests { ... }` — inline tests at the bottom.
  Stateless procedural primitives use unit structs (`pub(crate) struct ElfLoader;`) so the template still applies. Support modules (constants, decoders, top-level shared types) that aren't primitives are exempt from the template but still follow items 1 (module doc) and 2 (imports).
- **Function length:** Short is the default. There is no hard cap. A function that reads linearly top-to-bottom and is obvious at one glance can be 30–40 lines without harm. The test is comprehension, not line count: if you cannot follow what a function does in five seconds, restructure it (extract a clearly-named helper, fold a branch into a `match`, hoist common state). Don't extract helpers just to hit a number.
- **Naming:** Plain, functional, descriptive. No metaphor extensions. No abbreviations beyond well-established ones (`vm`, `kvm`, `mmio`).
- **Comments:** Comments explain why, not what. Code that needs a comment to explain what it does is rewritten to be self-explanatory.
- **Pure functions preferred:** Where state must be threaded, it is threaded explicitly through arguments and return values, not hidden in closures or globals. Classes exist only when state genuinely needs encapsulation; the default is functions over types over traits over generics.
- **Consistency is non-negotiable:** Similar things look similar. Different things look different. Each pattern in the project is defined once and followed everywhere; exceptions are bugs.

### 5.2 Dependencies

Every dependency is justified in writing in the project's design log. The bar for a new dependency is high. Standard library is preferred. `kvm-bindings` and `libc` are accepted as foundational. New dependencies require demonstrating that the alternative — writing the code in-tree — would be meaningfully worse.

### 5.3 Testing

- Unit tests for every public function in every subsystem.
- Property-based tests for every guest-controlled input parser.
- Integration tests that boot real kernels end-to-end.
- Fuzzing for all device implementations and the boot protocol parser.
- The 24-hour churn test runs nightly in CI from Phase 8 onward.
- No flaky tests. A flaky test is broken; it is fixed or removed.

### 5.4 Documentation

- This document is updated before code that contradicts it is written.
- Every subsystem has a one-page design document at `docs/<subsystem>.md`.
- Every public API function has rustdoc with a worked example.
- The threat model lives at `docs/threat-model.md` and is updated when boundaries change.
- The design log lives at `docs/decisions/` with one file per significant decision, dated, including the alternatives considered and why they were rejected.

### 5.5 Code review and decision-making

For a solo or small-team project, the discipline that replaces external review is:

- Every PR includes a written justification that references the constitution where relevant.
- Decisions that touch architecture require updating the relevant design document in the same PR.
- Decisions that violate the constitution require updating the constitution first, in a separate, deliberate change.

## 6. Success Criteria

### 6.1 Internal (the only ones that matter for project decisions)

- Ingot runs the agent workloads it was built for, with measurably stronger isolation than prior solutions.
- Any file in the project can be read on a phone screen in under five minutes and understood.
- The architecture documents written in Phase 0 still describe the code accurately at the end of Phase 10.
- Non-trivial changes in any subsystem can be made in under a day by someone familiar with the project.
- The 24-hour churn test passes consistently.

### 6.2 External (observed, not chased)

- Other engineers read the code and find it instructive.
- If anyone adopts Ingot, they do so for reasons the project author can name.
- Bug reports, when they arrive, are about real bugs, not about misunderstandings caused by sloppy documentation.

## 7. Failure Modes to Watch For

The following are documented because they are the most likely ways this project fails. Naming them in advance makes them harder to drift into.

### 7.1 Architectural perfectionism without empirical pressure

Spending months on abstractions that look elegant but cannot support the eventual goals. Mitigation: the falsification spikes in Phase 0 and Phase 9 are mandatory, not optional.

### 7.2 Scope creep through "small" features

Each individual addition seems minor; the cumulative effect is the cruft Ingot exists to avoid. Mitigation: the "every feature must come with a deletion" rule, and the binding nature of Section 2.5.

### 7.3 Premature daemonization

Building an HTTP control plane "in case someone needs it." Mitigation: the constitution clause forbidding this until concrete written requirements force it.

### 7.4 Drift toward feature parity with Firecracker

The temptation, around month 9, to close the visible gap. Mitigation: the anti-goals in Section 2.7, re-read quarterly.

### 7.5 Theme-park naming

Letting "Ingot" metastasize into `cast`, `forge`, `temper`, `quench` throughout the API. Mitigation: Section 1's naming rules, enforced in code review.

### 7.6 Snapshot/restore added late and badly

Bolting it on after the architecture solidifies, replicating Firecracker's `versionize` situation. Mitigation: it is in deferred scope, requires an RFC, and the RFC must address state-management implications across every existing subsystem before any code is written.

### 7.7 The bus factor of one

The project depends on its single maintainer for years. Mitigation: documentation is treated as the project's continuity plan. Anyone reading the docs should be able to continue the project.

## 8. Document Maintenance

This document is revised when:

- A foundational commitment changes. (Requires deliberate, dated revision.)
- A scope decision changes. (Requires moving the item between sections 2.5, 2.6, and the active scope, with justification.)
- A phase completes. (Mark complete, note actual duration vs. estimate, capture lessons.)
- A failure mode listed in section 7 manifests. (Document what happened and how it was addressed.)

Commits that contradict this document without a corresponding revision to the document are bugs.

## 9. Handoff Notes

This document is the project's ground truth. Read it before reading any code. Re-read sections 2 and 7 when tempted to deviate. The constitution exists because every clause in it represents a failure mode observed in adjacent projects; respecting them is how Ingot avoids becoming what it was built to be an alternative to.

The differentiators in section 2.4 are private. They do not appear in README, talks, or external communication until they are real. The public posture is: pedagogical reference implementation. The private aim is: substrate sharp enough to matter.

The first concrete action for the implementing engineer is Phase 0, section 4. Do not write production code before completing Phase 0's falsification spikes.

End of specification.
