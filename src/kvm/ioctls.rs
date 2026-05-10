//! KVM ioctl request numbers.
//!
//! Computed from the linux/include/uapi/asm-generic/ioctl.h encoding,
//! because kvm-bindings does not expose the ioctl macros — only the
//! struct shapes and the `KVM_EXIT_*` / `KVM_API_VERSION` constants.

use std::ffi::c_ulong;
use std::mem::size_of;

use kvm_bindings::{kvm_regs, kvm_sregs, kvm_userspace_memory_region};

const KVMIO: u32 = 0xAE;
const NR_BITS: u32 = 8;
const TYPE_BITS: u32 = 8;
const SIZE_BITS: u32 = 14;
const NR_SHIFT: u32 = 0;
const TYPE_SHIFT: u32 = NR_SHIFT + NR_BITS;
const SIZE_SHIFT: u32 = TYPE_SHIFT + TYPE_BITS;
const DIR_SHIFT: u32 = SIZE_SHIFT + SIZE_BITS;
const DIR_NONE: u32 = 0;
const DIR_WRITE: u32 = 1;
const DIR_READ: u32 = 2;

const fn ioc(dir: u32, ty: u32, nr: u32, size: u32) -> c_ulong {
    ((dir << DIR_SHIFT) | (size << SIZE_SHIFT) | (ty << TYPE_SHIFT) | (nr << NR_SHIFT)) as c_ulong
}

pub const KVM_GET_API_VERSION: c_ulong = ioc(DIR_NONE, KVMIO, 0x00, 0);
pub const KVM_CREATE_VM: c_ulong = ioc(DIR_NONE, KVMIO, 0x01, 0);
pub const KVM_GET_VCPU_MMAP_SIZE: c_ulong = ioc(DIR_NONE, KVMIO, 0x04, 0);
pub const KVM_CREATE_VCPU: c_ulong = ioc(DIR_NONE, KVMIO, 0x41, 0);
pub const KVM_RUN: c_ulong = ioc(DIR_NONE, KVMIO, 0x80, 0);

pub const KVM_SET_USER_MEMORY_REGION: c_ulong =
    ioc(DIR_WRITE, KVMIO, 0x46, size_of::<kvm_userspace_memory_region>() as u32);
pub const KVM_GET_REGS: c_ulong = ioc(DIR_READ, KVMIO, 0x81, size_of::<kvm_regs>() as u32);
pub const KVM_SET_REGS: c_ulong = ioc(DIR_WRITE, KVMIO, 0x82, size_of::<kvm_regs>() as u32);
pub const KVM_GET_SREGS: c_ulong = ioc(DIR_READ, KVMIO, 0x83, size_of::<kvm_sregs>() as u32);
pub const KVM_SET_SREGS: c_ulong = ioc(DIR_WRITE, KVMIO, 0x84, size_of::<kvm_sregs>() as u32);
