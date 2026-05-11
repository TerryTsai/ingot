# Threat model

This document is the deliverable required by [SPEC.md §5.4](../SPEC.md). It enumerates the trust boundaries the VMM crosses, the threats each boundary faces, and where in the code those threats are mitigated. Updated when boundaries change.

## 0. Status

Reflects committed code through Phase 4. Phase 5 (virtio-vsock) and Phase 7 (jailer) each add or modify a boundary; this document is updated in the same change as the code.

## 1. Boundaries

Four boundaries, each with its own contract. SPEC §3.3 is authoritative for the contract; this document expands each into adversary, threats, and mitigations.

### 1.1 Caller ↔ library

**Contract:** caller is trusted. Library validates inputs (paths exist, sizes are sane, configuration is internally consistent) but does not assume adversarial behavior from the caller. Panics are bugs.

**Adversary model:** a caller may pass malformed or oversized configuration *by mistake*, not by design. We surface clear errors, never panic, never silently truncate.

**Threats:**
- Caller passes nonexistent kernel or initramfs path. *Mitigated:* `Vm::validate` checks `is_file()` and returns `HostError::KernelImageNotReadable` / `InitramfsNotReadable` before any KVM call.
- Caller passes oversized cmdline. *Mitigated:* `Vm::validate` bounds cmdline length at 2048 bytes and rejects embedded NUL.
- Caller passes unreasonable memory size. *Mitigated:* minimum 16 MiB, must be page-aligned; reject otherwise.
- Caller passes vCPU count other than 1. *Mitigated:* Phase 2 explicitly rejects (multi-vCPU is Phase 6 work; failing fast prevents partial-init bugs).
- Library panics. *Mitigated:* `cargo test --quiet` covers the validation paths; the surface is small. Panics in tests fail CI.

### 1.2 VMM ↔ guest

**Contract:** the guest is hostile. Every byte read from guest memory is validated. Device implementations assume adversarial descriptor chains, malformed virtio queue state, malicious vsock payloads, integer overflow attempts, descriptors pointing outside guest memory, rings that loop forever.

**Adversary model:** a guest kernel + userspace controlled by an attacker who is trying to break out into the VMM process or the host. The guest's CPU is hardware-isolated by Intel VT-x, but its memory accesses through virtio devices flow through *our* code.

**Threats and mitigations:**
- **Descriptor index out of range.** A guest could write `avail.ring[i] = 65535` to make us index past the descriptor table. *Mitigated:* `VirtioQueue::pop_avail` and `VirtioQueue::read_desc` (src/devices/virtio_queue.rs) bounds-check `head < num` before dereferencing.
- **Descriptor chain forms a cycle.** A guest could set `desc[N].next = N` and trick us into an infinite loop. *Mitigated:* `virtio_chain::read_chain` and `virtio_chain::write_chain` cap iteration at the queue's `num` and return `GuestError::DescriptorChainCycle` if exceeded.
- **Descriptor `addr + len` overflows or exceeds guest memory.** A guest could request reading 2 GiB starting at address 2^63. *Mitigated:* `Memory::bounds_check` (src/memory.rs) uses `checked_add` and rejects spans that exceed the mapped region. *All* memory access from device code goes through this.
- **Driver-direction violation in descriptor chain.** A virtio-block READ request whose data descriptors are not marked `WRITE` would cause us to write guest data into our file. *Mitigated:* `block_io::do_io` checks `desc.flags & DESC_F_WRITE` matches the request direction; mismatch → `STATUS_IOERR` (no host I/O performed).
- **Malformed virtio-block header.** Guest sends a non-16-byte header. *Mitigated:* `block_io::split_chain` requires the first descriptor be exactly `HEADER_LEN` and read-only; otherwise `MalformedDescriptor`.
- **Out-of-range MMIO offset.** Guest writes past a device's MMIO region. *Mitigated:* `MmioBus::find` (src/devices/mmio_bus.rs) returns `MmioOutOfRange` if `addr + len > base + size`.
- **Guest fills RX queue with malicious "available" entries before sending TX.** Phase 4 path: if attacker arranges descriptors that point at sensitive guest-memory addresses, what we write there is what *the guest itself* wrote — no host data leaks. The scatter target is always guest physical memory the guest already controls.
- **Guest writes nonsense to virtio-mmio Status register.** *Mitigated:* `VirtioMmio::write_reg` round-trips the byte; no kernel state is changed by junk status writes.
- **Guest issues a `KVM_EXIT_IO` for an unexpected port** (e.g., legacy DMA controller). *Mitigated:* `Vm::handle_pio_in` returns 0xFF for anything not COM1 / CMOS data; `handle_pio_out` drops. No information leak, no behavioral effect.

**Non-defenses (explicit out-of-scope):**
- We do not mitigate Spectre / Meltdown class side-channel attacks beyond what the host kernel + microcode provide. SPEC §2.5 puts hardware-level isolation in the host's hands.
- We do not enforce timing isolation. A guest that runs a tight loop steals one vCPU's worth of host CPU. The jailer (Phase 7) is where cgroup CPU caps come in.
- We do not rate-limit virtio TX. A guest blasting packets at our TAP could saturate host networking. The jailer's network namespace plus host-side `tc` is the answer.

### 1.3 VMM ↔ host kernel

**Contract:** the host kernel is trusted, but the VMM process itself runs sandboxed.

**Adversary model:** in the Phase 7 jailer's world, the VMM is *suspect* even though it sits on the trusted side of the guest boundary. A bug in our virtio code could be exploited by a hostile guest to make the VMM perform syscalls it shouldn't. The seccomp filter limits the damage.

**Threats:**
- VMM is exploited via a virtio bug and tries to `unlinkat`, `execve`, `socket(AF_UNIX)`, etc. *Mitigation (Phase 7, not yet implemented):* seccomp BPF filter built from the syscall set we provably need: `read`, `write`, `pread64`, `pwrite64`, `mmap`, `munmap`, `ioctl` (allowlisted on `/dev/kvm` and TAP fds), `clock_gettime`, `nanosleep`, `futex`, `epoll_wait`. Adding a new syscall requires written justification.
- VMM holds privileges it doesn't need. *Mitigation (Phase 7):* drop everything but `CAP_NET_ADMIN` (only required briefly during TAP setup, then dropped) before entering the run loop.

### 1.4 Jailer ↔ host

**Contract:** the jailer runs with elevated privileges briefly to set up isolation primitives, then drops privileges and `execve`s the VMM binary.

**Phase 7, not yet implemented.** The boundary's job will be to make the privilege window the smallest possible: no long-lived elevated process.

## 2. Validation philosophy

Every guest-controlled input has *one* validation site, as close to the entry point as possible. Subsystems above it cannot forget the check because the type / value is sanitized before they see it.

- Guest memory bounds: `Memory::bounds_check` is the only path into the mapping. Every `Memory::read` and `Memory::write` goes through it.
- Descriptor indices: `VirtioQueue::pop_avail` and `read_desc`. Devices receive validated `Desc { addr, len, flags, next }` values, not raw indices.
- Descriptor chain depth: capped at `queue.num` in `virtio_chain::read_chain` / `write_chain`. No device can loop.
- MMIO range: `MmioBus::find`. Devices receive an offset relative to their registered base, validated to be within the region they registered.

Panics are bugs. Devices return `GuestError` for anything caused by guest behavior; the run loop treats that as a teardown reason, not a crash.

## 3. What changes when

| Code change | Threat-model section to revisit |
|---|---|
| New device on the MMIO bus | §1.2 — what its descriptor chain semantics are and where they're checked. Add to the "Threats and mitigations" list. |
| Touch `Memory::bounds_check` | §2 — the central guarantee comes from this function. Cross-check every caller. |
| Add a syscall to the allowlist | §1.3 — written justification per SPEC §3.3.3. |
| Phase 5 (vsock) | §1.2 — vsock packets are a new hostile-input parser. §3.6 (control vs data path) constrains how host-side socket fds reach the VMM. |
| Phase 7 (jailer) | §1.3 and §1.4 — entire sections fill in. |
