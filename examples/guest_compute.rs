//! Demo workload that runs *inside* the guest, not against ingot's
//! library API. Build with:
//!   rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
//!         examples/guest_compute.rs -o /tmp/guest-compute
//! Then pack into an initramfs and run via the CLI (see
//! scripts/compute-demo.sh).
//!
//! What it does:
//!   1. Prints a banner + warmup prime sieve to /dev/console (serial).
//!   2. Binds 0.0.0.0:9000 and serves the prime-count operation: each
//!      client connection reads a single newline-terminated integer N
//!      and replies with "primes up to N: count=X, largest=Y\n".
//! End-to-end: real compute + real Linux network stack + real TCP
//! over the virtio-net path that Phase 4 wired up.

use std::io::{Read, Write};
use std::net::TcpListener;

fn primes_up_to(n: usize) -> Vec<u32> {
    let mut sieve = vec![true; n + 1];
    if n >= 1 { sieve[0] = false; sieve[1] = false; }
    let limit = (n as f64).sqrt() as usize;
    for i in 2..=limit {
        if sieve[i] {
            for j in (i * i..=n).step_by(i) { sieve[j] = false; }
        }
    }
    sieve.iter().enumerate().filter_map(|(i, &p)| if p { Some(i as u32) } else { None }).collect()
}

fn handle(input: &str) -> String {
    let n: usize = input.trim().parse().unwrap_or(100);
    let n = n.min(10_000_000);
    let primes = primes_up_to(n);
    let largest = primes.last().copied().unwrap_or(0);
    format!("primes up to {}: count={}, largest={}\n", n, primes.len(), largest)
}

fn main() -> std::io::Result<()> {
    println!();
    println!("=========================================");
    println!("  ingot guest compute demo");
    println!("  warmup: prime sieve up to 1000");
    let primes = primes_up_to(1000);
    println!("  found {} primes, largest = {}", primes.len(), primes.last().unwrap());
    println!("=========================================");
    println!();
    let listener = TcpListener::bind("0.0.0.0:9000")?;
    println!("listening on 0.0.0.0:9000 (send N, get prime count)");
    println!();
    for stream in listener.incoming() {
        let mut s = stream?;
        let mut buf = [0u8; 64];
        let n_read = s.read(&mut buf)?;
        let input = std::str::from_utf8(&buf[..n_read]).unwrap_or("");
        let response = handle(input);
        print!("client: req={:?} -> {}", input.trim(), response);
        s.write_all(response.as_bytes())?;
    }
    Ok(())
}
