// Phase 4 placeholder. virtio-net: caller-provided TAP fd over MMIO virtqueues.
#![allow(unused_variables)]

use std::os::unix::io::RawFd;

use super::{GuestError, Interrupt, MmioDevice};
use crate::memory::Memory;

pub(crate) struct Net {
    _placeholder: (),
}

impl Net {
    pub fn new(tap_fd: RawFd, mem: &Memory) -> Self {
        unimplemented!()
    }
}

impl MmioDevice for Net {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        unimplemented!()
    }

    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError> {
        unimplemented!()
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        unimplemented!()
    }
}
