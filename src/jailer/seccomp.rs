//! Seccomp BPF allowlist for the VMM process. The VMM runs sandboxed
//! and the seccomp filter is the contract: exactly the syscalls we
//! provably need, no others. Adding one requires a written justification.
//!
//! Two install modes (CLI `--seccomp`):
//! - `log` — filter logs blocked syscalls to dmesg, allows them. Use
//!   during development to enumerate the allowlist.
//! - `strict` — kills the process on any syscall outside the list.
//!   Production posture once the allowlist is stable.

use std::io;

#[repr(C)]
struct SockFilter { code: u16, jt: u8, jf: u8, k: u32 }
#[repr(C)]
struct SockFprog { len: u16, filter: *const SockFilter }

const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;
const BPF_RET: u16 = 0x06;

const SECCOMP_MODE_FILTER: libc::c_int = 2;
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_RET_LOG: u32 = 0x7ffc_0000;
const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;
const PR_SET_SECCOMP: libc::c_int = 22;

#[derive(Copy, Clone)]
pub enum Mode { Log, Strict }

impl Mode {
    /// Install the filter. Must be called *after* all setup syscalls that
    /// aren't in `ALLOW` (e.g. open, socket, connect, bind) have happened.
    /// In practice: just before the Vm::run run loop, never earlier.
    pub fn install(self) -> io::Result<()> {
        let default = match self { Mode::Log => SECCOMP_RET_LOG, Mode::Strict => SECCOMP_RET_KILL_PROCESS };
        let mut prog: Vec<SockFilter> = Vec::with_capacity(ALLOW.len() * 2 + 2);
        // Load syscall_nr at seccomp_data offset 0
        prog.push(SockFilter { code: BPF_LD | BPF_W | BPF_ABS, jt: 0, jf: 0, k: 0 });
        for &nr in ALLOW {
            prog.push(SockFilter { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 0, jf: 1, k: nr as u32 });
            prog.push(SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW });
        }
        prog.push(SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: default });
        let fprog = SockFprog { len: prog.len() as u16, filter: prog.as_ptr() };
        let r = unsafe { libc::prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if r < 0 { return Err(io::Error::last_os_error()); }
        let r = unsafe { libc::prctl(PR_SET_SECCOMP, SECCOMP_MODE_FILTER, &fprog as *const _, 0, 0) };
        if r < 0 { return Err(io::Error::last_os_error()); }
        Ok(())
    }
}

/// Runtime allowlist for the VMM after Vm::new finishes. Each entry
/// MUST be justified — see the comment above its line. Order doesn't
/// affect correctness but does affect the dispatch cost; keep the hot
/// ones (ioctl, read, write) early. Numbers are x86_64.
const ALLOW: &[libc::c_long] = &[
    libc::SYS_ioctl,            // KVM_RUN, vhost-vsock, tap fcntl
    libc::SYS_read,             // tap, vsock eventfd, stdin thread
    libc::SYS_write,            // tap, stdout sink, vsock kick eventfd
    libc::SYS_pread64,
    libc::SYS_pwrite64,
    libc::SYS_futex,            // std::sync::Mutex
    libc::SYS_clock_gettime,    // Instant::now()
    libc::SYS_nanosleep,        // input-thread delay
    libc::SYS_rt_sigreturn,     // signal handler exit (SIGALRM)
    libc::SYS_rt_sigaction,     // install_wakeup_timer
    libc::SYS_rt_sigprocmask,   // panics on early stdlib paths
    libc::SYS_setitimer,        // wakeup timer
    libc::SYS_mmap,             // allocator slow path
    libc::SYS_munmap,           // drop
    libc::SYS_mprotect,         // allocator
    libc::SYS_brk,              // small alloc
    libc::SYS_close,            // OwnedFd drop
    libc::SYS_fcntl,            // tap O_NONBLOCK was already set, but...
    libc::SYS_exit,
    libc::SYS_exit_group,
    libc::SYS_gettid,            // panic handler
    libc::SYS_getpid,
    libc::SYS_writev,            // stderr buffered writes from eprintln
    libc::SYS_lseek,             // block file seeks
    libc::SYS_fstat,             // block file size lookup
    libc::SYS_sigaltstack,       // rust runtime signal handling
    libc::SYS_madvise,           // allocator
    libc::SYS_clock_nanosleep,   // thread::sleep on modern glibc
    libc::SYS_restart_syscall,   // kernel-injected restart of an EINTR'd syscall
    libc::SYS_poll,              // libc internal
    libc::SYS_ppoll,             // ditto
    libc::SYS_sched_yield,       // Rust mutex spin path
    // boot_kernel() runs between install and run_loop; it reads the kernel
    // ELF + optional initramfs off disk. These could be split out if the
    // jailer installs seccomp later, but keeping them in keeps the
    // single-binary install simple.
    libc::SYS_openat,
    libc::SYS_statx,
    libc::SYS_newfstatat,
    libc::SYS_getrandom,         // Rust HashMap seed
];

