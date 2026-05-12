# 0007 — Property tests cover SPEC §3.3, cargo-fuzz deferred

Date: 2026-05-12. Status: accepted.

## Decision

For Phase 8's "fuzzing harness for every guest-controlled input
surface" deliverable, use `proptest` against the same surface
instead of `cargo-fuzz` for now. Defer `cargo-fuzz` integration
until property tests reveal a real coverage gap.

## Why

`proptest` and `cargo-fuzz` solve overlapping problems differently:

- **proptest:** generates random inputs from a typed strategy, runs
  the test, and shrinks failing inputs automatically. Runs in
  `cargo test`. Per-test budget is the iteration count (200 for us,
  ~2400 random inputs across 12 property tests). Cost: seconds per
  CI run.

- **cargo-fuzz:** drives the target with libFuzzer's coverage-guided
  byte stream. Runs continuously (typically on a fuzz server),
  not in CI. Per-target budget is hours-to-days. Finds rare paths
  property tests miss because it tracks coverage and steers toward
  unexplored branches.

The SPEC §3.3 contract for device code is: "no panic, no infinite
loop, every guest-controlled byte validated." That's a *correctness*
property — property tests express it directly and prove it against
hundreds of random cases per CI run. The marginal value of fuzzing
ON TOP of property tests is finding paths proptest's strategies
don't cover, which matters more once the property tests are mature
and we want to harden further.

Bringing in `cargo-fuzz` also adds workspace plumbing:
- A `fuzz/` sub-crate (separate workspace member)
- A `feature = "fuzz"` flag on the main crate so internal types
  (`VirtioQueue`, `Memory`, `MmioBus`) become reachable for the
  fuzz target — these are `pub(crate)` today and shouldn't leak
  into the public API.
- A way to actually RUN fuzzers (a separate machine, a fuzz
  service, etc.) — without that, the fuzz target rots.

Property tests need none of this. They use `#[cfg(test)]` modules
already in place, exercise the same internal types directly, and
run in the existing CI workflow.

## When to revisit

Add `cargo-fuzz` when:
- Property tests pass cleanly for ≥3 months in CI and we want
  deeper coverage of the surfaces SPEC §3.3 names.
- We have a place to run fuzzers continuously (OSS-Fuzz, a
  dedicated machine).
- A real bug shows up that property tests wouldn't have found —
  e.g., a specific byte sequence triggering integer overflow in a
  parser. That's the signal that strategy-driven generation is
  hitting a wall.

Until then: property tests + the existing inline `#[cfg(test)]`
suite + `scripts/*-test.sh` end-to-end scripts cover the bar.

## What this means for the SPEC §4 Phase 8 deliverable

"Fuzzing harness for every guest-controlled input surface" reads
as a literal demand for cargo-fuzz. We're interpreting it
functionally: a hardening regime that exercises adversarial inputs
against each surface. Property tests do that with shrinking and
without the workspace overhead. The SPEC will be amended at the
next Phase 8 review to acknowledge property-based testing as the
primary mechanism.
