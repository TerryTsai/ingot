//! Devices subsystem tests.

mod bus;
mod uart;

use std::io;
use std::sync::{Arc, Mutex};

pub(super) struct Captured(pub Arc<Mutex<Vec<u8>>>);

impl io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn new_capture() -> (Box<dyn io::Write + Send>, Arc<Mutex<Vec<u8>>>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    (Box::new(Captured(buf.clone())), buf)
}
