# Spike: typed harness

Tests architecture.md §12.2: a typed task harness (manifest-producing, dry-run-capable) sits naturally on top of `Vm::new`/`Vm::run` plus a caller-owned vsock listener.

## Method

Sketch a small harness library on top of Ingot. Define a `Task` trait with output/error associated types, plus `Host`, `Plan`, `Run`, and `Manifest`. Implement two concrete tasks (`EvalTask`, `CommandTask`) to verify the trait shape generalizes across two task families that share little surface. Stay near 200 lines per SPEC.md §4 Phase 0.

## Sketch

[`examples/harness/`](../../examples/harness):

| File      | Lines | Role                                                  |
|-----------|-------|-------------------------------------------------------|
| `main.rs` |  ~40  | Entry point; constructs and runs both tasks.          |
| `task.rs` |  ~55  | `Task` trait, `Host`, `Plan`, `Run`, `Manifest`.      |
| `eval.rs` |  ~70  | `EvalTask` impl: source code in, stdout/exit out.     |
| `cmd.rs`  |  ~55  | `CommandTask` impl: program+stdin in, stdout/exit out.|

The sketch compiles against the Phase 0 API. `execute()` bodies are `unimplemented!()` and contain inline notes pointing to the ergonomics-spike frictions that block them.

## Findings

### Confirmation: §10 holds under the harness shape

The harness uses only the public API (`Vm::new`, `Vm::run`) and a caller-owned vsock listener. No proxying through the VMM. No internal hooks. The data path is entirely the harness's concern: it speaks to the guest over vsock to deliver inputs and collect outputs, and combines that with the VM-level `Outcome` to build a `Manifest`. The architectural separation between control plane (Ingot) and data plane (harness's vsock) is precisely what makes the harness layer cheap.

### Confirmation: dry-run is a harness-layer property

`Plan::plan()` is a pure function of the task's own state. No Ingot involvement. The harness can describe what `execute()` would do without touching KVM. Nothing in the API constrains this — it falls out of where `plan()` lives.

### Confirmation: `Manifest` composes from existing types

`Manifest` combines (a) harness-side measurements — byte counts on the vsock channel, harness-side wall-clock — with (b) VMM-side `ExitStatus` and `Telemetry`. Both halves are reachable through the public API. Manifests are the harness's authoritative record of *what the harness saw and chose to record*; the VMM is not asked to participate.

### Confirmation: the trait shape generalizes

The two concrete tasks share `Host` and the trait's associated-type pattern. Their `execute()` skeletons diverge only in the wire payload, not in the surrounding ceremony. Adding a third task family (e.g., a build job) would follow the same shape.

### No new frictions beyond the ergonomics spike

The five frictions surfaced in [embedded-api-ergonomics.md](embedded-api-ergonomics.md) — timeout, blocking `Vm::run`, AF_VSOCK ergonomics, UART/workload conflation, code-delivery convention — all apply here. Building the harness exposes the same gaps; it does not introduce new ones.

### Specifically *not* needed: per-syscall audit log

Architecture.md §12.2 listed "per-syscall audit log" as a possible `Outcome` field. The harness does not need it. The auditable surface the harness cares about is what crossed the vsock channel — bytes in, bytes out, payloads — and that is fully under the harness's control. Guest-internal syscalls are a guest concern.

### One small additional ask: derive `Debug` on public types

The harness's `Manifest` carries `ExitStatus` and would naturally include `Telemetry`. Neither type derives `Debug` today. Harnesses that want to log or pretty-print manifests have to write per-field formatters. Adding `#[derive(Debug)]` (and `Clone` where structurally cheap) on the public outcome types is a low-cost quality-of-life change. Not blocking.

## Verdict

**Confirmed.** The Phase 0 architecture supports the typed-harness shape without violating §10 or requiring new internal hooks. Once the five ergonomics-spike API changes land, the `execute()` bodies become naturally sized rather than 60-80-line ceremony.

No additions to `Outcome` or `Config` are required beyond what the ergonomics spike already proposed, plus the small ask above (derive `Debug`/`Clone` on public outcome types).
