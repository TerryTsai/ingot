use super::{GuestError, Interrupt, MmioDevice};
use crate::config::StdoutSink;

pub(crate) struct Uart {
    _placeholder: (),
}

impl Uart {
    pub fn new(sink: StdoutSink) -> Self {
        unimplemented!()
    }
}

impl MmioDevice for Uart {
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
