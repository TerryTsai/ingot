use std::os::unix::io::RawFd;

use super::{GuestError, Interrupt, MmioDevice};
use crate::guest_mem::GuestMem;

pub(crate) struct Vsock {
    _placeholder: (),
}

impl Vsock {
    pub fn new(host_listener_fd: RawFd, guest_cid: u32, mem: &GuestMem) -> Self {
        unimplemented!()
    }
}

impl MmioDevice for Vsock {
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
