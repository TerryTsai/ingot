//! Jailer subsystem. Two halves:
//!
//!   In-process (always linked into the VMM):
//!     seccomp.rs   the BPF allowlist the VMM installs on itself
//!                  before entering its run loop.
//!
//!   Out-of-process (the `ingot-jailer` binary, src/bin/ingot-jailer.rs):
//!     cgroups.rs    cgroups v2 limit setup
//!     namespaces.rs mount/PID/network/IPC namespace unshare
//!     mount.rs      bind mounts + pivot_root into a minimal rootfs
//!     privilege.rs  drop UID/GID, set PR_SET_NO_NEW_PRIVS
//!
//! `jail_and_exec` chains the four out-of-process primitives in their
//! canonical order — cgroup → unshare → mount/pivot → drop privs →
//! execve — and replaces the current process with the VMM binary.
//! Each step requires the previous; the jailer must run as root and
//! drops privilege only after every operation that needed it.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::process::ExitCode;

mod cgroups;
mod mount;
mod namespaces;
mod privilege;
mod seccomp;

pub use cgroups::Limits;
pub use mount::BindMount;
pub use seccomp::Mode;

pub struct JailConfig {
    /// Name of the cgroup to create under /sys/fs/cgroup/.
    pub cgroup_name: String,
    pub limits: Limits,
    pub uid: u32,
    pub gid: u32,
    /// Supplementary groups to retain across the privilege drop.
    /// `[kvm_gid]` keeps /dev/kvm accessible; `[]` is strictest.
    pub supp_groups: Vec<u32>,
    /// Path on the host that will become the VMM's new root.
    pub new_root: PathBuf,
    /// Bind mounts placed inside `new_root` before pivot_root.
    pub binds: Vec<BindMount>,
    /// Path to the `ingot` binary to execve.
    pub ingot_path: PathBuf,
    /// Args to pass to `ingot` after argv[0].
    pub forwarded_args: Vec<String>,
}

/// Set up the jail and run the VMM. Order is fixed:
///   1. cgroup join — limits apply to everything that follows.
///   2. unshare namespaces + fork — child inherits the new mount/PID/
///      net/IPC namespaces and becomes PID 1 of the new PID namespace.
///   3. (child) mount + pivot_root — `/proc` is mounted with the new
///      PID namespace, so it shows only the jail's processes.
///   4. (child) privilege drop — after every root-requiring step.
///   5. (child) execve the VMM.
///   6. (parent) wait for child, exit with its status.
///
/// CLONE_NEWPID requires the fork because the namespace only takes
/// effect for the unsharer's children, not the unsharer itself.
pub fn jail_and_exec(config: JailConfig) -> io::Result<ExitCode> {
    cgroups::Cgroup::create(&config.cgroup_name, &config.limits)?;
    match namespaces::Namespaces::unshare_and_fork()? {
        namespaces::Forked::Child => {
            mount::Mount::contain(&config.new_root, &config.binds)?;
            privilege::Privilege::drop_to(config.uid, config.gid, &config.supp_groups)?;
            let _: std::convert::Infallible = exec_into(&config.ingot_path, &config.forwarded_args)?;
            unreachable!()
        }
        namespaces::Forked::Parent { child_pid } => {
            namespaces::Namespaces::wait_for_child(child_pid)
        }
    }
}

/// `execv` the ingot binary, building argv from `[binary_path, ...args]`.
/// Only returns on error; on success the process image is replaced.
fn exec_into(binary: &std::path::Path, args: &[String]) -> io::Result<std::convert::Infallible> {
    let prog = CString::new(binary.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "binary path contains NUL"))?;
    let mut argv_c: Vec<CString> = Vec::with_capacity(args.len() + 1);
    argv_c.push(prog.clone());
    for a in args {
        argv_c.push(CString::new(a.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "arg contains NUL"))?);
    }
    let mut argv_ptrs: Vec<*const libc::c_char> = argv_c.iter().map(|s| s.as_ptr()).collect();
    argv_ptrs.push(std::ptr::null());
    unsafe { libc::execv(prog.as_ptr(), argv_ptrs.as_ptr()); }
    Err(io::Error::last_os_error())
}
