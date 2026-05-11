# ingot

A minimal KVM-based virtual machine monitor for short-lived, hardware-isolated compute. Pedagogical reference implementation in Rust.

## What it does

Boots a Linux microVM, runs a workload to completion, captures the result, tears the VM down. Library-first; the CLI is a thin wrapper.

The build target is a microVM small enough to fit on one screen of `ls`. Today: ~1500 lines of Rust + a 13 MB custom kernel + ~700 KB compressed initramfs.

## What works today

Phases 2 through 5 of the build plan in [SPEC.md §4](SPEC.md) are shipped. Five reproducible test scripts in `scripts/` each print eight PASS/FAIL indicators.

| Phase | What | Test |
|---|---|---|
| 2 | Boots a real Linux kernel to an interactive shell over serial. | manual |
| 3 | virtio-block: file-backed `/dev/vda`, ext4 mount + read + write + persistence. Strict done: boot from disk, no initramfs. | `scripts/block-test.sh`, `scripts/disk-boot.sh` |
| 4 | virtio-net: TAP-backed `eth0` with full TCP/IP, ping to host works. | `scripts/net-test.sh` |
| 5 | virtio-vsock: AF_VSOCK ↔ AF_VSOCK round-trip between host and guest, vhost-vsock passthrough. | `scripts/vsock-test.sh` |
| — | End-to-end compute demo: static Rust binary as PID 1, serves prime-sieve compute to the host over TCP. | `scripts/compute-demo.sh` |

77 unit tests + 1 doctest pass on real KVM.

## Quick start

Prerequisites: Linux x86_64 host with `/dev/kvm`, user in the `kvm` group, Rust toolchain, `fakeroot` and `e2fsprogs` for the test scripts. The test scripts also need `sudo` for `tap0` setup and `mkfs.ext4`.

Build a custom microvm-tuned kernel (see [docs/phase2-real-kernel.md](docs/phase2-real-kernel.md) for the config fragment) and a busybox-static. Then any of:

```sh
cargo test --quiet            # 77 unit + 1 doctest on real KVM
./scripts/net-test.sh         # virtio-net round-trip
./scripts/block-test.sh       # virtio-block read/write
./scripts/vsock-test.sh       # AF_VSOCK echo round-trip
./scripts/disk-boot.sh        # boot from disk (no initramfs)
./scripts/compute-demo.sh     # static Rust /init serves compute over TCP
```

Manual run:

```sh
cargo run --bin ingot -- run \
  --kernel /tmp/kbuild/linux-6.6.30/vmlinux \
  --initramfs /tmp/initramfs.cpio.gz \
  --memory 256 \
  --tap tap0 \
  --rootfs /tmp/disk.img \
  --cmdline "console=ttyS0,115200 virtio_mmio.device=4K@0xd0000000:5 virtio_mmio.device=4K@0xd0001000:6 reboot=k panic=1"
```

## Layout

- [`SPEC.md`](SPEC.md) — the binding specification. Phases, scope discipline, anti-goals.
- [`docs/architecture.md`](docs/architecture.md) — subsystem boundaries, interfaces, run-loop design.
- [`docs/threat-model.md`](docs/threat-model.md) — trust boundaries and what hostile-guest protection looks like in code.
- [`docs/phase2-real-kernel.md`](docs/phase2-real-kernel.md) — kernel build journal, what's required to make this boot.
- [`docs/decisions/`](docs/decisions/) — one file per load-bearing design choice. Five decisions logged so far.
- `src/kvm/` — `/dev/kvm` ioctl wrappers.
- `src/boot/` — Linux x86_64 boot protocol (ELF load, page tables, GDT, boot_params, MP table).
- `src/devices/` — virtio-mmio transport + virtio-block + virtio-net + 8250 UART + CMOS shim.
- `src/vm.rs` — orchestrator. State machine, run loop, MMIO/PIO dispatch.

## Status

Phases 6–10 are not yet started. Phase 6 (library API / lifecycle polish) is largely a side-effect of Phases 2–5 — the real next chunks are Phase 7 (jailer: seccomp + namespaces + cgroups, separate binary) and the Phase 9 differentiator spikes.

## License

Apache 2.0.
