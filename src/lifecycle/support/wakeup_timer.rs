//! SIGALRM-driven KVM_RUN wakeup. `Vm::run_loop` installs this once
//! before entering the loop. With no SA_RESTART, every SIGALRM fires
//! `KVM_RUN` back to userspace with `-EINTR`, which `Vcpu::run` turns
//! into `VcpuExit::Intr`. The run loop then drives `poll_io` (TAP RX,
//! vhost call eventfds) before re-entering `KVM_RUN`. Without this, a
//! guest sitting in `hlt` would never let the host drain its async fds
//! because nothing else generates a vCPU exit.

/// Install the SIGALRM handler and arm `setitimer` at 5 ms intervals.
/// Idempotent only in the sense that re-installing replaces the prior
/// handler/timer — the run loop only calls this once.
pub(crate) fn install() {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigalrm_noop as *const () as libc::sighandler_t;
        sa.sa_flags = 0;
        libc::sigemptyset(&mut sa.sa_mask);
        libc::sigaction(libc::SIGALRM, &sa, std::ptr::null_mut());
        let it = libc::itimerval {
            it_interval: libc::timeval { tv_sec: 0, tv_usec: 5_000 },
            it_value:    libc::timeval { tv_sec: 0, tv_usec: 5_000 },
        };
        libc::setitimer(libc::ITIMER_REAL, &it, std::ptr::null_mut());
    }
}

// `extern "C"` thunk referenced from sigaction. Has to be a top-level
// function for the C ABI; the body is intentionally empty (the SIGALRM
// only exists to make KVM_RUN return -EINTR).
extern "C" fn sigalrm_noop(_: libc::c_int) {}
