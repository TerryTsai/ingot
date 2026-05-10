use std::path::Path;

use super::{GuestError, Interrupt, MmioDevice};
use crate::guest_mem::GuestMem;

pub(crate) struct Block {
    _placeholder: (),
}

impl Block {
    pub fn new(path: &Path, readonly: bool, mem: &GuestMem) -> Self {
        unimplemented!()
    }
}

impl MmioDevice for Block {
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
