//! AF_VSOCK echo server, runs inside the guest as PID 1. Standalone
//! Rust — no `libc` crate, just raw `extern "C"` to the musl syscalls
//! we need, so a one-shot `rustc --target x86_64-unknown-linux-musl`
//! works without setting up cargo.
//!
//! Binds VMADDR_CID_ANY:9000; per accept, reads one packet and writes
//! "ECHO: <payload>" back.

use std::os::raw::{c_int, c_void};

const AF_VSOCK: c_int = 40;
const SOCK_STREAM: c_int = 1;
const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;
const PORT: u32 = 9000;

#[repr(C)]
struct SockaddrVm {
    family: u16, reserved1: u16,
    port: u32, cid: u32, zero: [u8; 4],
}

extern "C" {
    fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const c_void, len: u32) -> c_int;
    fn listen(fd: c_int, backlog: c_int) -> c_int;
    fn accept(fd: c_int, addr: *mut c_void, len: *mut u32) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    fn close(fd: c_int) -> c_int;
}

fn die(what: &str) -> ! {
    eprintln!("vsock-echo: {what} failed: errno={}", std::io::Error::last_os_error());
    std::process::exit(1);
}

fn main() {
    println!();
    println!("=========================================");
    println!("  ingot guest vsock echo server");
    println!("  AF_VSOCK on port {PORT}");
    println!("=========================================");
    println!();

    let lfd = unsafe { socket(AF_VSOCK, SOCK_STREAM, 0) };
    if lfd < 0 { die("socket"); }
    let addr = SockaddrVm {
        family: AF_VSOCK as u16, reserved1: 0,
        port: PORT, cid: VMADDR_CID_ANY, zero: [0; 4],
    };
    if unsafe { bind(lfd, &addr as *const _ as _, std::mem::size_of::<SockaddrVm>() as _) } < 0 { die("bind"); }
    if unsafe { listen(lfd, 4) } < 0 { die("listen"); }
    println!("listening (waiting for host connection)");

    loop {
        let cfd = unsafe { accept(lfd, std::ptr::null_mut(), std::ptr::null_mut()) };
        if cfd < 0 { die("accept"); }
        let mut buf = [0u8; 1024];
        let n = unsafe { read(cfd, buf.as_mut_ptr() as _, buf.len()) };
        if n <= 0 { unsafe { close(cfd); } continue; }
        let pfx = b"ECHO: ";
        unsafe { write(cfd, pfx.as_ptr() as _, pfx.len()); }
        unsafe { write(cfd, buf.as_ptr() as _, n as usize); }
        let s = String::from_utf8_lossy(&buf[..n as usize]);
        println!("client said {:?} ({} bytes)", s.trim(), n);
        unsafe { close(cfd); }
    }
}
