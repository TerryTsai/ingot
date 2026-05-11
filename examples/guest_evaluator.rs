//! Guest-side compute service. Accepts one AF_VSOCK connection, reads
//! a single integer N, replies with "<count of primes up to N>\n", then
//! exits. PID 1 exit triggers panic + reboot which the host's
//! `Vm::run` translates to `ExitStatus::GuestFault(TripleFault)`.
//!
//! Build: rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
//!        examples/guest_evaluator.rs -o /tmp/guest-evaluator

use std::os::raw::{c_int, c_void};

const AF_VSOCK: c_int = 40;
const SOCK_STREAM: c_int = 1;
const VMADDR_CID_ANY: u32 = 0xFFFF_FFFF;
const PORT: u32 = 9100;

#[repr(C)]
struct SockaddrVm {
    family: u16, reserved1: u16,
    port: u32, cid: u32, zero: [u8; 4],
}

extern "C" {
    fn socket(d: c_int, t: c_int, p: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const c_void, len: u32) -> c_int;
    fn listen(fd: c_int, backlog: c_int) -> c_int;
    fn accept(fd: c_int, addr: *mut c_void, len: *mut u32) -> c_int;
    fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
}

fn primes_up_to(n: u64) -> u64 {
    if n < 2 { return 0; }
    let mut sieve = vec![true; (n + 1) as usize];
    sieve[0] = false; sieve[1] = false;
    let mut i: u64 = 2;
    while i * i <= n {
        if sieve[i as usize] {
            let mut j = i * i;
            while j <= n { sieve[j as usize] = false; j += i; }
        }
        i += 1;
    }
    sieve.iter().filter(|&&p| p).count() as u64
}

fn main() {
    let lfd = unsafe { socket(AF_VSOCK, SOCK_STREAM, 0) };
    if lfd < 0 { eprintln!("socket: {}", std::io::Error::last_os_error()); return; }
    let addr = SockaddrVm {
        family: AF_VSOCK as u16, reserved1: 0,
        port: PORT, cid: VMADDR_CID_ANY, zero: [0; 4],
    };
    if unsafe { bind(lfd, &addr as *const _ as _, std::mem::size_of::<SockaddrVm>() as _) } < 0 {
        eprintln!("bind: {}", std::io::Error::last_os_error()); return;
    }
    if unsafe { listen(lfd, 1) } < 0 {
        eprintln!("listen: {}", std::io::Error::last_os_error()); return;
    }
    println!("evaluator: listening on vsock:*:{PORT}");

    let cfd = unsafe { accept(lfd, std::ptr::null_mut(), std::ptr::null_mut()) };
    if cfd < 0 { eprintln!("accept: {}", std::io::Error::last_os_error()); return; }
    let mut buf = [0u8; 64];
    let n_read = unsafe { read(cfd, buf.as_mut_ptr() as _, buf.len()) };
    if n_read <= 0 { return; }
    let req: u64 = std::str::from_utf8(&buf[..n_read as usize])
        .unwrap_or("").trim().parse().unwrap_or(0);
    let count = primes_up_to(req);
    let resp = format!("{count}\n");
    unsafe { write(cfd, resp.as_ptr() as _, resp.len()); }
    // Give the kernel a moment to flush the response through vhost-vsock
    // before init exits — otherwise the host's read can race with the
    // kernel tearing down the connection on panic.
    std::thread::sleep(std::time::Duration::from_millis(100));
    println!("evaluator: req={req} -> count={count}; exiting");
}
