# Ingot

Minimal KVM-based VMM in Rust for ephemeral, hardware-isolated microVMs running single-task workloads.

The canonical specification is **[SPEC.md](SPEC.md)**. It is binding. Re-read it (especially sections 2 and 7) before non-trivial work. Deviation requires revising SPEC.md *first*, in a separate change — not commits that contradict it.

## Operating constraints (excerpted from SPEC.md — the spec is authoritative)

### Code style (§5.1)

- No file exceeds 100 lines. Splits are by responsibility, not line count.
- No function exceeds 20 lines.
- Plain functional naming. No metaphor extensions: never `smelt`, `forge`, `cast`, `temper`, `quench`. The name "Ingot" is the only metaphor.
- CLI verbs are plain English (`run`, `inspect`, `kill`).
- Comments explain *why*, not *what*. Default to no comments.
- Functions over types over traits over generics. Thread state explicitly through arguments and return values; no hidden globals or closures.
- No abbreviations beyond well-established ones (`vm`, `kvm`, `mmio`).
- Standard library preferred. New dependencies require written justification in `docs/decisions/`.

### Scope (§2.5–2.7)

- **Library first, daemon never** until concrete written requirements force it. The CLI is a thin wrapper over the library.
- Every feature must come with either a corresponding deletion or written justification.
- Permanently out of scope: non-Linux guests, GPU/USB/PCI passthrough, PCI transport, live migration, multi-tenant density, memory hot-plug/ballooning, MMDS, CPU templates.
- Deferred (requires RFC): aarch64, snapshot/restore, multi-VM-per-process, daemon control plane, virtio-fs, io_uring.
- Anti-goals: feature parity with Firecracker, "production-ready for arbitrary users", adding things because someone asked / it'd be cool / it'd be easy.

### Process (§4, §5.5)

- Phases are sequential. No phase begins until the previous phase's done criterion is met.
- **Phase 0 (design + four falsification spikes) must complete before any production code is written.**
- Every PR includes a written justification referencing the constitution where relevant.
- Architecture changes update the relevant design document in the same change.

### Threat model (§3.3)

- The guest is hostile. Every byte read from guest memory is validated. Device code assumes adversarial descriptor chains, malicious payloads, integer overflows, descriptors pointing outside guest memory.
- The caller is trusted; the library validates inputs but does not assume adversarial callers. Panics are bugs.
- The VMM is sandboxed by the jailer's seccomp filter. Adding a permitted syscall requires written justification.
- No "trusted mode" bypasses ever — including for the project author's own workloads.

### Differentiators (§2.4 — PRIVATE)

The four axes (cold boot <50ms, embedded API ergonomics, agent-harness support, auditability) are private aims. They do **not** appear in README, talks, or external communication until they are real. Public posture: pedagogical reference implementation.

## Layout

The spec mandates this layout. None of these paths exist yet.

- `SPEC.md` — canonical specification. Binding.
- `docs/<subsystem>.md` — one-page design doc per subsystem (`kvm`, `boot`, `devices`, `lifecycle`, `jailer`).
- `docs/threat-model.md` — threat model. Updated when boundaries change.
- `docs/decisions/` — design log. One file per significant decision, dated, with alternatives considered and why they were rejected.

## Status

Phase 0 — design and falsification. No production code yet. The first concrete action is the Phase 0 deliverables in SPEC.md §4.
