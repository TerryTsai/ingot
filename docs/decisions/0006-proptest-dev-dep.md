# 0006 — proptest as dev-dependency

Date: 2026-05-12. Status: accepted.

## Decision

Add `proptest` 1.x as a `[dev-dependencies]` entry. Property-based
tests use it; production code does not. The dependency is invisible
in the release binary.

## Why

SPEC §3.3 says the guest is hostile and every device's input surface
must assume "adversarial descriptor chains, malicious payloads,
integer overflows, descriptors pointing outside guest memory, rings
that loop forever." SPEC §4 Phase 8 says "property-based testing
extended to all device implementations."

Hand-written `#[cfg(test)]` tests cover the *known* adversarial
patterns (cyclic descriptor chains, out-of-bounds heads, malformed
descriptors). They don't cover *unknown* patterns. Property tests
generate inputs we wouldn't think of and run thousands of
iterations per test, catching panics or invariant violations that
unit tests miss.

A property test for `VirtioQueue::read_chain` says: "for any byte
pattern in guest memory and any head index, `read_chain` either
returns a `GuestError` or a `Vec<u8>`; it never panics, never loops
infinitely, and the returned `Vec<u8>` length is bounded by
`num * max_desc_len`." This is the kind of statement SPEC §3.3
requires, and `proptest` is the standard Rust tool for stating it.

## Alternatives considered

- **`quickcheck`** — older, less actively maintained, no shrink
  tracing on async. proptest has been the de-facto choice for
  property testing in Rust since ~2020.
- **Hand-rolled random testing.** Could use `rand` + a loop. Misses
  shrinking (the killer feature of proptest: when a test fails,
  proptest minimizes the failing input automatically). Without
  shrinking the failure reports are pages of random bytes, useless
  for debugging.
- **Fuzzing only** (no proptest). Fuzzing is great but it's a
  separate workflow — runs continuously on a fuzz server, not in
  per-commit CI. Property tests are CI-cheap (seconds) and catch
  most issues earlier.

## Cost

`proptest` 1.x pulls in `rand` and a few small crates (`bit-set`,
`bit-vec`, `unarray`). All MIT/Apache-2.0. `cargo build` time
unaffected for release builds because it's dev-only.

## Scope

Strictly for tests inside `#[cfg(test)]` blocks. No `proptest` use
in production code paths. No public API exposes `proptest` types.
