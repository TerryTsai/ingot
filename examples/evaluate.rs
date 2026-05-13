//! Embedded-API worked example: `evaluate(input) -> output`, with
//! AF_VSOCK as the data channel. Spawn `Vm::run` on a background
//! thread, poll the guest's AF_VSOCK listener until reachable, send
//! one integer, read the prime-count reply, join the VM thread.
//!
//! Call as:
//!   cargo run --release --example evaluate -- <kernel> <initramfs> <cid> <input>
//!
//! Pairs with `examples/guest_evaluator.rs` packed as `/init` in the
//! initramfs.

use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use ingot::{Config, IngotError, StdoutSink, Vm, VsockSpec};

const AF_VSOCK: i32 = 40;
const PORT: u32 = 9100;

#[repr(C)]
struct SockaddrVm {
    family: u16, reserved1: u16,
    port: u32, cid: u32, zero: [u8; 4],
}

fn connect_vsock(cid: u32, port: u32, timeout_ms: u64) -> std::io::Result<std::fs::File> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        let fd = unsafe { libc::socket(AF_VSOCK, libc::SOCK_STREAM, 0) };
        if fd < 0 { return Err(std::io::Error::last_os_error()); }
        let addr = SockaddrVm {
            family: AF_VSOCK as u16, reserved1: 0,
            port, cid, zero: [0; 4],
        };
        let rc = unsafe {
            libc::connect(fd, &addr as *const _ as _, std::mem::size_of::<SockaddrVm>() as _)
        };
        if rc == 0 {
            let owned = unsafe { OwnedFd::from_raw_fd(fd) };
            let raw = owned.as_raw_fd();
            std::mem::forget(owned);
            return Ok(unsafe { std::fs::File::from_raw_fd(raw) });
        }
        unsafe { libc::close(fd); }
        thread::sleep(Duration::from_millis(50));
    }
    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "vsock connect timed out"))
}

fn evaluate(kernel: PathBuf, initramfs: PathBuf, cid: u32, input: String) -> Result<String, IngotError> {
    let cfg = Config {
        kernel_path: kernel,
        initramfs_path: Some(initramfs),
        vcpus: 1,
        memory_bytes: 128 * 1024 * 1024,
        cmdline: "console=ttyS0,115200 quiet virtio_mmio.device=4K@0xd0002000:7 reboot=t panic=1".into(),
        stdout: StdoutSink::Discard,
        vsock: Some(VsockSpec { guest_cid: cid }),
        ..Config::default()
    };
    // Spawn the AF_VSOCK client on a background thread; main thread
    // runs the VM (Vm holds raw pointers and isn't Send). The client
    // thread polls until the guest's listener accepts, then exchanges
    // bytes. Joining returns the response string.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let client = thread::spawn(move || {
        let mut conn = match connect_vsock(cid, PORT, 8000) {
            Ok(c) => c,
            Err(_) => { let _ = tx.send(String::from("ERR: connect timeout")); return; }
        };
        let _ = conn.write_all(input.as_bytes());
        let _ = conn.write_all(b"\n");
        let mut buf = String::new();
        let _ = conn.read_to_string(&mut buf);
        let _ = tx.send(buf.trim().to_string());
    });
    let _outcome = Vm::new(cfg)?.run()?;
    let _ = client.join();
    Ok(rx.try_recv().unwrap_or_else(|_| String::from("ERR: no reply")))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        eprintln!("usage: evaluate <kernel> <initramfs> <cid> <input>");
        std::process::exit(2);
    }
    match evaluate(PathBuf::from(&args[1]), PathBuf::from(&args[2]),
                   args[3].parse().unwrap(), args[4].clone()) {
        Ok(out) => println!("{out}"),
        Err(e) => { eprintln!("evaluate failed: {e:?}"); std::process::exit(1); }
    }
}
