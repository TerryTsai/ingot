//! Guest physical memory mapping.
//!
//! A single mmap-backed region [0, size). MMIO is outside this range
//! and is not backed by GuestMem.

use crate::devices::GuestError;
use crate::error::HostError;

pub struct GuestMem {
    _placeholder: (),
}

impl GuestMem {
    pub fn new(size_bytes: u64) -> Result<Self, HostError> {
        unimplemented!()
    }

    pub fn write(&mut self, guest_phys: u64, data: &[u8]) -> Result<(), GuestError> {
        unimplemented!()
    }

    pub fn read(&self, guest_phys: u64, dst: &mut [u8]) -> Result<(), GuestError> {
        unimplemented!()
    }

    pub fn host_ptr(&self) -> *mut u8 {
        unimplemented!()
    }

    pub fn size(&self) -> u64 {
        unimplemented!()
    }
}
