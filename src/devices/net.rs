use std::os::unix::io::RawFd;

use super::{GuestError, Interrupt, MmioDevice};
use crate::guest_mem::GuestMem;

pub(crate) struct Net {
    _placeholder: (),
}

impl Net {
    pub fn new(tap_fd: RawFd, mem: &GuestMem) -> Self {
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
