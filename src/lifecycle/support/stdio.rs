//! Host-side I/O wiring for the guest's UART. Builds the output sink
//! per `StdoutSink` config, wraps it to record first-output time, and
//! optionally spawns a thread pumping host stdin into the UART input
//! queue.
//!
//! Used only by `Vm::new` during construction; nothing here outlives
//! the Vm.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::FromRawFd;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::devices::{Uart, UartInput};
use crate::lifecycle::config::StdoutSink;
use crate::lifecycle::error::{HostError, IngotError};

pub(crate) type Captured = Option<Arc<Mutex<Vec<u8>>>>;
pub(crate) type FirstOutput = Arc<Mutex<Option<Instant>>>;

/// Construct a UART wired to the host: the configured stdout sink
/// (possibly captured to a buffer or duped to an fd), wrapped to mark
/// first-output time for telemetry. Spawns a stdin pump if interactive.
pub(crate) fn build_uart(
    sink_spec: &StdoutSink,
    interactive: bool,
) -> Result<(Uart, Captured, FirstOutput), IngotError> {
    let (inner, captured) = build_sink(sink_spec)?;
    let first_output: FirstOutput = Arc::new(Mutex::new(None));
    let sink: Box<dyn Write + Send> = Box::new(FirstWriteSink { inner, first_at: first_output.clone() });
    let uart = Uart::new(sink);
    spawn_stdin_pump(uart.input_handle(), interactive);
    Ok((uart, captured, first_output))
}

fn build_sink(s: &StdoutSink) -> Result<(Box<dyn Write + Send>, Captured), HostError> {
    Ok(match s {
        StdoutSink::Discard => (Box::new(io::sink()), None),
        StdoutSink::File(p) => (
            Box::new(File::create(p).map_err(|_| HostError::InvalidConfig("could not open stdout file"))?),
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
    if d < 0 { return Err(HostError::InvalidConfig("dup failed for StdoutSink::Fd")); }
    Ok(unsafe { File::from_raw_fd(d) })
}

/// When `interactive`, spawn a detached thread reading host stdin one
/// byte at a time and pushing into the UART's input queue. The thread
/// exits when stdin EOFs or errors. Detached because library callers
/// using interactive=true accept the leak.
///
/// When stdin is not a TTY (i.e. piped), pause for 2s before pushing.
/// Piped input would otherwise saturate the UART queue while the
/// kernel is still booting; when sh later opens `/dev/console`, the
/// n_tty ldisc resets its read buffer and discards bytes that were
/// already there. Real interactive typing is naturally paced and
/// doesn't trip this — so when stdin is a TTY we don't wait.
fn spawn_stdin_pump(input: UartInput, interactive: bool) {
    if !interactive { return; }
    std::thread::spawn(move || {
        if unsafe { libc::isatty(0) } == 0 {
            std::thread::sleep(std::time::Duration::from_millis(2000));
        }
        let stdin = io::stdin();
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

struct BufferSink(Arc<Mutex<Vec<u8>>>);
impl Write for BufferSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

/// Sink wrapper that records the moment of the first non-empty write.
/// Used to mark `Telemetry::first_output_at` for cold-boot measurement.
struct FirstWriteSink {
    inner: Box<dyn Write + Send>,
    first_at: Arc<Mutex<Option<Instant>>>,
}
impl Write for FirstWriteSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !buf.is_empty() {
            let mut g = self.first_at.lock().unwrap();
            if g.is_none() { *g = Some(Instant::now()); }
        }
        self.inner.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}
