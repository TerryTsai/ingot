//! CPUID table passed to KVM. Wraps the C-style flex-array layout of
//! `struct kvm_cpuid2` with a fixed-size entries array so the whole
//! thing is a single heap allocation.

use kvm_bindings::kvm_cpuid_entry2;

const MAX_CPUID_ENTRIES: usize = 256;

#[repr(C)]
pub struct Cpuid {
    nent: u32,
    padding: u32,
    entries: [kvm_cpuid_entry2; MAX_CPUID_ENTRIES],
}

impl Cpuid {
    /// Allocate a zeroed CPUID buffer on the heap, capacity-set to the
    /// maximum number of entries we'll accept from KVM.
    pub fn new() -> Box<Self> {
        let mut b: Box<Self> = unsafe { Box::<Self>::new_zeroed().assume_init() };
        b.nent = MAX_CPUID_ENTRIES as u32;
        b
    }

    pub fn nent(&self) -> u32 {
        self.nent
    }
}
