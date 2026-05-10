//! KVM subsystem tests.
//!
//! These tests require /dev/kvm to be readable. When it is not, each test
//! prints a skip message via stderr and passes — `cargo test` therefore
//! succeeds locally even without KVM access. On a properly configured host
//! they exercise every public function in the kvm subsystem, including the
//! Phase 1 done-criterion 4 KiB real-mode hello-world.

mod hello;
mod host;
mod vcpu;
mod vm;

use super::Kvm;

pub(super) fn open_or_skip(test: &str) -> Option<Kvm> {
    match Kvm::open() {
        Ok(k) => Some(k),
        Err(e) => {
            eprintln!("[{}] skipped: cannot open /dev/kvm ({:?})", test, e);
            None
        }
    }
}
