# 0004: SIGALRM wakeup timer for the run loop

Date: 2026-05-11
Status: accepted
Phase: 4

## Decision

The lifecycle layer installs a no-op SIGALRM handler and arms
`setitimer(ITIMER_REAL, 5ms)` once at run-loop start. SIGALRM has
`sa_flags = 0` (no SA_RESTART), so `KVM_RUN` returns `-EINTR` on every
delivery. `VcpuFd::run` maps that errno to the new `VcpuExit::Intr`
variant; the run loop treats it as a no-op exit that still cycles
through `poll_io` and IRQ reconcile.

## Why

Without this, a guest sitting in `hlt` never produces a vCPU exit, so
`bus.poll_io` never runs and the host TAP fd backs up with frames the
guest is waiting for. Concretely: ping seq=0 worked because the guest
was busy sending — TX exits drove `poll_io` calls that happened to find
the reply on the TAP. Subsequent pings hung because the guest hlt'd
between sends and our run loop blocked in `KVM_RUN` while replies
accumulated unread.

The 5ms cadence is well below any application timeout that matters
(ARP retries are 1s, TCP RTO starts at 200ms minimum) and well above
the rate at which a 1ms timer would generate gratuitous wakeups for an
otherwise-idle guest.

## Alternatives rejected

- **Spawn an RX thread that reads TAP and signals via `KVM_IRQFD` or
  `KVM_IRQ_LINE`**: the canonical pattern in QEMU and crosvm. Requires
  wrapping `Net` in `Arc<Mutex<…>>`, sharing `VmFd` with the worker,
  and threading guest memory access across threads. Worth doing when
  we add a second async-backed device, but it's ~150 lines of refactor
  for one device.
- **`poll()` with a short timeout before each `KVM_RUN`**: only catches
  packets that arrive between exits. Doesn't help the *during-hlt*
  case, which is the actual gap.
- **Eventfd-based wakeup driven by a separate poller**: same refactor
  cost as the thread approach, just with a different signal mechanism.
- **Use `KVM_SET_SIGNAL_MASK` to scope the unmasked signal more
  tightly**: SIGALRM is the only signal we install a handler for and
  the only one we arm — no scoping needed. Default global mask works.

## Cost paid

- One process-global signal handler. The handler does nothing; the
  state change is entirely the syscall-interruption side effect.
- One process-global `ITIMER_REAL`. Won't conflict with the guest
  because the guest's own SIGALRM is a virtualized signal inside the
  guest kernel, not the host's timer.
- ~12 extra `set_irq_line` calls per second from spurious wake-ups when
  the guest is idle. Negligible.

## When to revisit

- When we add a second async-backed device (vsock, future virtio-fs,
  etc.) and the threading refactor becomes more justified.
- If profiling shows the wake-ups eating measurable CPU.
- If we add adaptive tick rate (busy: 1ms; idle: 50ms+) to optimize
  the cold-boot-latency differentiator.
