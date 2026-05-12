//! Error type for the kvm subsystem and the two helpers (`last_errno`,
//! `check_ioctl`) every ioctl wrapper goes through. `check_ioctl`
//! converts a libc ioctl return value into `Result<i32, KvmError>`,
//! tagging the failure with the ioctl name for diagnostics.

use std::io;

#[derive(Debug)]
pub enum KvmError {
    Open(i32),
    AbiVersionMismatch { got: i32, want: u32 },
    Ioctl { name: &'static str, errno: i32 },
    Mmap(i32),
    UnhandledExit(u32),
}

impl KvmError {
    pub(crate) fn last_errno() -> i32 {
        io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    /// Convert an ioctl return value to `Result`. Non-negative values
    /// pass through; negative ones become `KvmError::Ioctl` with the
    /// current errno and the ioctl name for diagnostics.
    pub(crate) fn check_ioctl(r: libc::c_int, name: &'static str) -> Result<libc::c_int, KvmError> {
        if r < 0 {
            Err(KvmError::Ioctl { name, errno: Self::last_errno() })
        } else {
            Ok(r)
        }
    }
}
