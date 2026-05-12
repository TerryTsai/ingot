//! KVM ioctl wrappers. The three Fd primitives (`KvmFd`, `VmFd`,
//! `VcpuFd`) own their respective KVM file descriptors and expose the
//! minimum surface the orchestrator needs. The hierarchy mirrors the
//! kernel's: KvmFd opens /dev/kvm and creates VmFds; each VmFd creates
//! VcpuFds; each VcpuFd runs the guest until a `VcpuExit`.

mod cpuid;
mod error;
mod ioctls;
mod kvm_fd;
mod vcpu_fd;
mod vm_fd;

pub(crate) use cpuid::Cpuid;
pub use error::KvmError;
pub(crate) use kvm_fd::KvmFd;
pub(crate) use vcpu_fd::VcpuExit;
pub(crate) use vcpu_fd::VcpuFd;
pub(crate) use vm_fd::VmFd;
