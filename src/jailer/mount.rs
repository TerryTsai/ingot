//! Mount-namespace setup. After `Namespaces::unshare` puts us in a
//! private mount namespace, `Mount::contain` builds a minimal rootfs
//! containing only the device nodes and files the VMM declared, then
//! `pivot_root`s into it. The result: the VMM cannot see any host file
//! outside the declared set.
//!
//! Sequence (every step matters):
//!   1. Mark root recursively private so our mounts don't propagate.
//!   2. Bind-mount the new root to itself with MS_BIND (pivot_root
//!      requires the new root to be a mount point distinct from old).
//!   3. Bind-mount each declared file/dir into the new root.
//!   4. Mount /proc inside the new root (needs new PID namespace
//!      already unshared so it shows only this jail's processes).
//!   5. `pivot_root` (new_root, new_root/oldroot).
//!   6. `umount2(oldroot, MNT_DETACH)` to remove every trace of the
//!      old mount tree.

use std::ffi::CString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub struct Mount;

impl Mount {
    /// Build a minimal rootfs at `root`, bind-mount `binds` into it,
    /// and pivot_root. After this returns, the process's view of the
    /// filesystem is rooted at `root`'s contents — host paths outside
    /// `binds` are unreachable.
    pub fn contain(root: &Path, binds: &[BindMount]) -> io::Result<()> {
        Self::mark_root_private()?;
        Self::ensure_dir(root)?;
        Self::bind(root, root)?;
        for b in binds {
            Self::install_bind(root, b)?;
        }
        let proc_path = root.join("proc");
        Self::ensure_dir(&proc_path)?;
        Self::mount_proc(&proc_path)?;
        Self::pivot_into(root)
    }

    fn mark_root_private() -> io::Result<()> {
        let target = CString::new("/").unwrap();
        let r = unsafe {
            libc::mount(
                std::ptr::null(),
                target.as_ptr(),
                std::ptr::null(),
                libc::MS_REC | libc::MS_PRIVATE,
                std::ptr::null(),
            )
        };
        if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    fn bind(src: &Path, dst: &Path) -> io::Result<()> {
        let s = path_to_c(src);
        let d = path_to_c(dst);
        let r = unsafe {
            libc::mount(s.as_ptr(), d.as_ptr(), std::ptr::null(), libc::MS_BIND, std::ptr::null())
        };
        if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// Install a single bind into the new root. `b.source` (host path)
    /// becomes visible at `root + b.target`. Creates the target file or
    /// directory as needed.
    fn install_bind(root: &Path, b: &BindMount) -> io::Result<()> {
        let target = root.join(b.target.strip_prefix("/").unwrap_or(&b.target));
        if let Some(parent) = target.parent() { fs::create_dir_all(parent)?; }
        let src_meta = fs::metadata(&b.source)?;
        if src_meta.is_dir() {
            fs::create_dir_all(&target)?;
        } else {
            fs::OpenOptions::new().create(true).write(true).truncate(false).open(&target)?;
        }
        Self::bind(&b.source, &target)
    }

    fn mount_proc(target: &Path) -> io::Result<()> {
        let src = CString::new("proc").unwrap();
        let typ = CString::new("proc").unwrap();
        let t = path_to_c(target);
        let r = unsafe {
            libc::mount(src.as_ptr(), t.as_ptr(), typ.as_ptr(), 0, std::ptr::null())
        };
        if r < 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// `pivot_root(new_root, new_root/oldroot)`, then chdir + detach old.
    fn pivot_into(new_root: &Path) -> io::Result<()> {
        let old = new_root.join("oldroot");
        fs::create_dir_all(&old)?;
        let new = path_to_c(new_root);
        let old_c = path_to_c(&old);
        let r = unsafe { libc::syscall(libc::SYS_pivot_root, new.as_ptr(), old_c.as_ptr()) };
        if r < 0 { return Err(io::Error::last_os_error()); }
        let slash = CString::new("/").unwrap();
        if unsafe { libc::chdir(slash.as_ptr()) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let oldroot = CString::new("/oldroot").unwrap();
        if unsafe { libc::umount2(oldroot.as_ptr(), libc::MNT_DETACH) } < 0 {
            return Err(io::Error::last_os_error());
        }
        fs::remove_dir("/oldroot").ok();
        Ok(())
    }

    fn ensure_dir(p: &Path) -> io::Result<()> {
        match fs::create_dir_all(p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e),
        }
    }
}

pub struct BindMount {
    /// Path on the host to bind in.
    pub source: PathBuf,
    /// Path inside the new root (e.g. "/dev/kvm", "/vmlinux").
    pub target: PathBuf,
}

fn path_to_c(p: &Path) -> CString {
    CString::new(p.as_os_str().as_bytes()).expect("path contains NUL")
}
