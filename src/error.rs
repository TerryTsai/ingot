use std::path::PathBuf;

use crate::devices::GuestError;
use crate::kvm::KvmError;

#[derive(Debug)]
pub enum IngotError {
    Kvm(KvmError),
    Guest(GuestError),
    Host(HostError),
}

impl From<KvmError> for IngotError {
    fn from(e: KvmError) -> Self {
        IngotError::Kvm(e)
    }
}

impl From<GuestError> for IngotError {
    fn from(e: GuestError) -> Self {
        IngotError::Guest(e)
    }
}

impl From<HostError> for IngotError {
    fn from(e: HostError) -> Self {
        IngotError::Host(e)
    }
}

#[derive(Debug)]
pub enum HostError {
    KernelImageNotReadable(PathBuf),
    InitramfsNotReadable(PathBuf),
    RootfsNotReadable(PathBuf),
    InvalidConfig(&'static str),
    MmapFailed(i32),
    TapCreateFailed(i32),
    SeccompInstallFailed(i32),
    CgroupSetupFailed(i32),
}
