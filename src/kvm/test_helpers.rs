//! Shared helpers for the kvm subsystem's inline test modules.

use super::KvmFd;

pub(crate) fn open_or_skip(test: &str) -> Option<KvmFd> {
    match KvmFd::open() {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("[{}] skipped: cannot open /dev/kvm ({:?})", test, e);
            None
        }
    }
}
