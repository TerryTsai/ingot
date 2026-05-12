//! KVM ioctl wrappers. Three primitives — `Kvm`, `Vm`, `Vcpu` —
//! each owning the corresponding kernel object's file descriptor. The
//! hierarchy mirrors the kernel's: `Kvm` opens `/dev/kvm` and creates
//! `Vm`s; each `Vm` creates `Vcpu`s; each `Vcpu` runs the guest until
//! a `VcpuExit`. The fd is an internal detail of each type; the type
//! name reflects the kernel object it represents.

mod error;
// `kvm::Kvm` is the principal handle; per the layout rule (file name =
// snake_case of struct), it lives in `kvm.rs`. The resulting `kvm/kvm`
// path triggers clippy's module_inception lint, which we suppress —
// the convention here is intentional.
#[allow(clippy::module_inception)]
mod kvm;
mod support;
mod vcpu;
mod vm;

pub use error::KvmError;
pub(crate) use kvm::Kvm;
pub(crate) use support::cpuid::Cpuid;
pub(crate) use vcpu::{Vcpu, VcpuExit};
pub(crate) use vm::Vm;
