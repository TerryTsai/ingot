//! Host-side I/O for the UART: output sinks built from `StdoutSink`
//! plus an optional stdin reader thread when `Config::interactive`.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;

use crate::config::StdoutSink;
use crate::devices::UartInput;
use crate::error::HostError;

pub(super) fn make_sink(s: &StdoutSink) -> Result<Box<dyn Write + Send>, HostError> {
    Ok(match s {
        StdoutSink::Discard => Box::new(std::io::sink()),
        StdoutSink::File(p) => Box::new(
            File::create(p).map_err(|_| HostError::InvalidConfig("could not open stdout file"))?,
        ),
        StdoutSink::Fd(fd) => Box::new(dup_to_file(*fd)?),
        StdoutSink::Buffer => {
            return Err(HostError::InvalidConfig("StdoutSink::Buffer not yet supported"));
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

/// When `interactive`, spawn a detached thread reading host stdin one
/// byte at a time and pushing into the UART's input queue. The thread
/// exits when stdin reaches EOF, when read errors, or when the only
/// remaining reference to the queue is the thread itself (UART dropped).
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
