// Phase 3 placeholder. virtio-block: file-backed disk over MMIO virtqueues.
#![allow(unused_variables)]

use std::path::Path;

use super::{GuestError, Interrupt, MmioDevice};
use crate::memory::Memory;

pub(crate) struct Block {
    _placeholder: (),
}

impl Block {
    pub fn new(path: &Path, readonly: bool, mem: &Memory) -> Self {
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
