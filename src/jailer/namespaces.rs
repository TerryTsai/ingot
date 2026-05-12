//! Linux namespace setup. `unshare` four namespaces and fork. The four:
//!   - mount    isolated /proc, /sys, no view of host filesystem
//!   - PID      cannot see or signal host processes
//!   - network  no host interfaces unless explicitly passed
//!   - IPC      separate System V IPC, POSIX MQs
//!
//! `unshare(CLONE_NEWPID)` only takes effect for the caller's *children*,
//! not the caller itself. So the canonical pattern is: unshare in the
//! parent, fork, child inherits all the new namespaces and runs the
//! rest of the jail setup as PID 1 of the new namespace; parent waits
//! and exits with the child's status.
//!
//! User namespaces are intentionally NOT unshared. Running unprivileged
//! is achieved via setresuid/gid after the jailer's elevated setup work
//! is complete. Mixing user-ns with seccomp adds quirks (no_new_privs
//! interactions) we don't need.

use std::io;
use std::process::ExitCode;

const CLONE_NEWNS:  libc::c_int = 0x00020000;
const CLONE_NEWPID: libc::c_int = 0x20000000;
const CLONE_NEWNET: libc::c_int = 0x40000000;
const CLONE_NEWIPC: libc::c_int = 0x08000000;

pub struct Namespaces;

pub enum Forked {
    /// Returned in the child: it lives in the new mount/PID/net/IPC
    /// namespaces and should proceed with the rest of jail setup.
    Child,
    /// Returned in the parent: child's pid for waitpid.
    Parent { child_pid: libc::pid_t },
}

impl Namespaces {
    /// Unshare mount/PID/net/IPC, then fork. Returns the role of the
    /// caller after fork. Parent should wait for child.
    pub fn unshare_and_fork() -> io::Result<Forked> {
        let flags = CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWNET | CLONE_NEWIPC;
        if unsafe { libc::unshare(flags) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let pid = unsafe { libc::fork() };
        if pid < 0 { return Err(io::Error::last_os_error()); }
        if pid == 0 { Ok(Forked::Child) } else { Ok(Forked::Parent { child_pid: pid }) }
    }

    /// Block until `child_pid` exits, then return its exit code (or
    /// 128+signal for signal exits). Used by the parent after fork.
    pub fn wait_for_child(child_pid: libc::pid_t) -> io::Result<ExitCode> {
        let mut status: libc::c_int = 0;
        let r = unsafe { libc::waitpid(child_pid, &mut status, 0) };
        if r < 0 { return Err(io::Error::last_os_error()); }
        let code = if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status) as u8
        } else if libc::WIFSIGNALED(status) {
            128u8.saturating_add(libc::WTERMSIG(status) as u8)
        } else {
            1
        };
        Ok(ExitCode::from(code))
    }
}
