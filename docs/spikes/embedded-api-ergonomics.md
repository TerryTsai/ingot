# Spike: embedded API ergonomics

Tests the claim in [architecture.md §12.4](../architecture.md): rewriting a small real program against the Phase 0 API takes single-digit lines for the common case and surfaces no surprise frictions.

## Method

Pick the use case the project was built for: a short-lived sandbox that runs untrusted code, captures stdout, and returns. Write the call-site against the Phase 0 API. Note where the API makes the harness awkward, missing, or surprising. Recommend additions / changes that resolve the friction without violating the constitution.

## Target program: `ingot-eval`

Reads a program on stdin, executes it inside an Ingot VM with a prebuilt eval-rootfs image, captures the workload's stdout (kernel boot output is noise), enforces a wall-clock timeout, and reports `(exit_code, stdout)`.

## Attempt 1 — simple shape, via `StdoutSink::Buffer`

See [`examples/eval.rs`](../../examples/eval.rs). It compiles. The function body is roughly:

```rust
fn evaluate(_code: &str) -> Result<String, IngotError> {
    let cfg = Config {
        kernel_path: PathBuf::from("/var/lib/ingot/vmlinux"),
        initramfs_path: Some(PathBuf::from("/var/lib/ingot/eval-rootfs.cpio")),
        vcpus: 1,
        memory_bytes: 128 * 1024 * 1024,
        cmdline: "console=ttyS0 reboot=k panic=1 quiet".into(),
        stdout: StdoutSink::Buffer,
        ..Config::default()
    };
    let outcome = Vm::new(cfg)?.run()?;
    Ok(String::from_utf8_lossy(&outcome.stdout).into_owned())
}
```

This is the three-line shape the claim asks for, plus configuration. **It does not work as a code-runner.** The function ignores `code` because the API has no path to deliver it. The cmdline field is capped at ~2 KB by validation rules (architecture.md §7.1) and shell-escaping arbitrary user input into a kernel command line is unsafe even when it fits.

## Attempt 2 — realistic shape, via vsock

The architecture commits in §10 to caller-owned vsock for the data path. The realistic harness becomes:

```text
1. open AF_VSOCK socket, bind to (CID_ANY, port=42), listen
2. build VsockSpec { host_listener_fd, guest_cid: 3 }
3. build Config with vsock + initramfs + ...
4. spawn Vm::run on a worker thread (it blocks)
5. accept() the guest connection
6. write `code` to the connection
7. read workload stdout from the connection
8. join the worker thread, get Outcome
9. cross-check VM-level Outcome with workload-level result from the connection
```

Roughly 60-80 lines including the AF_VSOCK setup (no `std::os::unix::net` support), the threading, and the result merge. Not three lines. Not single-digit lines.

## Frictions

1. **No timeout.** `Config` has no `timeout` field. Every realistic harness needs a wall-clock cap on the guest. Caller-side workarounds are awkward because `Vm::run` consumes the `Vm` by value and exposes no cancel handle.

2. **`Vm::run` blocks with no cancel handle.** Concurrent vsock I/O during execution forces a thread + share-nothing pattern; the lack of cancellation compounds the timeout problem.

3. **AF_VSOCK setup is rough in std-Rust.** `std::os::unix::net` does not expose AF_VSOCK. Callers either drop to `libc::socket` directly or pull in a dependency. The library could create the listener internally given a port, hand the fd back, and leave §10's caller-owned semantics intact.

4. **UART output and workload output are conflated.** `StdoutSink::Buffer` captures the UART, which carries kernel boot messages alongside any early-init output. For a code-runner the relevant stream is the workload's stdout — which travels over vsock by the architecture's own logic. The naming `Outcome::stdout` is misleading.

5. **Code delivery has no clear path.** Cmdline is too small and unsafe. Initramfs is too slow to rebuild per invocation. Block device requires a filesystem image. Vsock requires the caller to drive a protocol the guest must speak. The library is silent on which is the conventional approach.

6. **No workload exit-code surface.** `ExitStatus::Halted` reports that the kernel halted, not whether the workload exited 0. The harness must read this from vsock. That is consistent with §10 — the data path is the caller's — but the docs and `Outcome` shape do not yet say so.

## Recommended changes

Each recommendation is tied to a friction. Each compiles a one-line rationale. None requires revising SPEC.md.

1. **Add `Config::timeout: Option<Duration>`.** Library enforces; on expiry, transition to `Exiting` with `ExitStatus::TimedOut`. (#1)

2. **Split `Vm::run` into `Vm::start` → `Handle::wait` / `Handle::cancel`.** Keep the blocking shape available via `Vm::start().wait()`; add cancellation and pre-`wait` introspection. The three-line shape becomes `Vm::new(cfg)?.start()?.wait()?`. (#1, #2)

3. **Allow `VsockSpec` to be constructed by port, not just by fd.** Add `VsockSpec::Bind { port: u32, guest_cid: u32 }` alongside the existing pre-built-fd form. The library binds and listens; the caller can retrieve the fd via `Handle::vsock_listener()` if it wants ownership. Caller-owned semantics intact, ergonomics better. (#3)

4. **Rename `Outcome::stdout` → `Outcome::uart`.** Be explicit: this is what the kernel and early init wrote to the UART. Workload output belongs on vsock. (#4)

5. **Document the conventional code-delivery pattern; do not bake it in.** A worked example (this spike, plus a Phase 6 example) showing vsock-based delivery with a tiny init shim the caller ships in their initramfs. The library does not gain a "send code to guest" knob — that would violate §10. (#5)

6. **Keep `ExitStatus` VM-level only.** Add `TimedOut` (per #1). Do not add `Workload(i32)` — workload exit code is the harness's concern, read over vsock. Document this explicitly. (#6)

## Verdict

**Refuted, with actionable changes.** The simple-case three-line example does compile, but it is not a working harness — it has no code-delivery path, no timeout, no concurrency story. The five API changes above bring the realistic harness within the original ergonomic claim without violating any constitutional rule.

These changes do not require revising SPEC.md. They do require revising architecture.md §4 (public API shape) and §6 (lifecycle, to admit `start`/`Handle`). Those revisions land alongside the implementation in Phase 6, not now.
