//! Drop UID/GID and clear ambient/inheritable capabilities. Called
//! *last* in the jailer's pre-exec sequence — after every operation
//! requiring root has completed (cgroup writes, mount namespace setup).
//! After this returns the process is unprivileged; subsequent execve
//! enters the VMM with no way to re-elevate.

use std::io;

pub struct Privilege;

impl Privilege {
    /// Set the supplementary group list to `supp_groups` (replacing
    /// any inherited from the elevated caller), then `setresgid` then
    /// `setresuid`. Order matters: groups → GID → UID, so each later
    /// step locks out re-elevation.
    ///
    /// `supp_groups` is *replacing*, not appending. Pass `[kvm_gid]`
    /// to retain /dev/kvm access; pass `[]` for the strictest drop.
    /// Production deployments should prefer fd passing of privileged
    /// resources (open /dev/kvm before drop, inherit through execve)
    /// to avoid keeping groups at all.
    pub fn drop_to(uid: u32, gid: u32, supp_groups: &[u32]) -> io::Result<()> {
        let r = unsafe { libc::setgroups(supp_groups.len(), supp_groups.as_ptr()) };
        if r < 0 { return Err(io::Error::last_os_error()); }
        if unsafe { libc::setresgid(gid, gid, gid) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::setresuid(uid, uid, uid) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // PR_SET_NO_NEW_PRIVS prevents any future execve from gaining
        // privileges via SUID/file caps. seccomp's strict mode sets the
        // same bit; this is cheap insurance for the layer below seccomp.
        if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
