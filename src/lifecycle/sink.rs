//! Host-side I/O for the UART: output sinks built from `StdoutSink`,
//! the in-memory capture path for `StdoutSink::Buffer`, plus an
//! optional stdin reader thread when `Config::interactive`.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::sync::{Arc, Mutex};

use crate::config::StdoutSink;
use crate::devices::UartInput;
use crate::error::HostError;

pub(super) type Captured = Option<Arc<Mutex<Vec<u8>>>>;

struct BufferSink(Arc<Mutex<Vec<u8>>>);

impl Write for BufferSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn make_sink(
    s: &StdoutSink,
) -> Result<(Box<dyn Write + Send>, Captured), HostError> {
    Ok(match s {
        StdoutSink::Discard => (Box::new(io::sink()), None),
        StdoutSink::File(p) => (
            Box::new(
                File::create(p)
                    .map_err(|_| HostError::InvalidConfig("could not open stdout file"))?,
            ),
            None,
        ),
        StdoutSink::Fd(fd) => (Box::new(dup_to_file(*fd)?), None),
        StdoutSink::Buffer => {
            let b = Arc::new(Mutex::new(Vec::new()));
            (Box::new(BufferSink(b.clone())), Some(b))
        }
    })
}

fn dup_to_file(fd: std::os::fd::RawFd) -> Result<File, HostError> {
    let d = unsafe { libc::dup(fd) };
    if d < 0 {
        return Err(HostError::InvalidConfig("dup failed for StdoutSink::Fd"));
    }
    Ok(unsafe { File::from_raw_fd(d) })
}

pub(super) fn maybe_spawn_stdin(input: UartInput, interactive: bool) {
    if !interactive {
        return;
    }
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut buf = [0u8; 1];
        loop {
            match handle.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => input.push(buf[0]),
            }
        }
    });
}
