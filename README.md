# ingot

A minimal KVM-based virtual machine monitor for short-lived, hardware-isolated compute. Written in Rust. Pedagogical reference implementation — small enough to read in a sitting.

## Overview

Ingot boots a Linux microVM, runs a single workload to completion, captures the result, and tears the VM down. The library is the product; the CLI is a thin wrapper.

Ingot has a minimalist design. It excludes anything not required to run a single short-lived guest: no live migration, no hot-plug, no PCI passthrough, no daemon control plane. The smaller surface reduces startup time, memory footprint, and the amount of code a reader has to hold in their head.

Today: ~3,700 lines of Rust + a custom Linux 6.6 kernel + a busybox initramfs.

## Quick start

Prerequisites: Linux x86_64 host with `/dev/kvm`, user in the `kvm` group, Rust toolchain, `fakeroot` and `e2fsprogs` for the test scripts. The test scripts also need `sudo` for `tap0` setup and `mkfs.ext4`.

Build the guest kernel and a static busybox (see [Guest kernel](#guest-kernel) below), then any of:

```sh
cargo test --quiet            # 97 unit tests + 1 doctest on real KVM
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

## What works

| Area | Status |
|---|---|
| Boot a real Linux kernel to an interactive shell over serial | ✓ |
| virtio-block: file-backed `/dev/vda`, ext4 mount/read/write/persistence, boot from disk | ✓ |
| virtio-net: TAP-backed `eth0` with full TCP/IP, ping to host | ✓ |
| virtio-vsock: AF_VSOCK ↔ AF_VSOCK between host and guest via vhost-vsock | ✓ |
| End-to-end compute demo: static Rust binary as PID 1, serves prime-sieve over TCP | ✓ |
| Jailer: seccomp + namespaces + cgroups + privilege drop, separate binary | ✓ |
| Snapshot / restore | out of scope |
| Multiple VMs per process, daemon control plane | out of scope |

97 unit tests and 1 doctest pass on real KVM. Each script in `scripts/` prints eight PASS/FAIL indicators.

## Guest kernel

Ingot runs a custom-built Linux 6.6. The full stripped-down `.config` is committed at [`kernel/config`](kernel/config) — a single file you drop into a Linux source tree and build. Resulting `vmlinux` is ~13 MB. Modules, PCI, ACPI, USB, sound, framebuffer, KASLR, audit, KEYS, and every filesystem except `devtmpfs`/`tmpfs`/`procfs`/`sysfs`/`ext4` are off.

Build:

```sh
curl -O https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.6.30.tar.xz
tar xf linux-6.6.30.tar.xz
cp kernel/config linux-6.6.30/.config
cd linux-6.6.30
make olddefconfig
make -j$(nproc) vmlinux
```

Build deps: `gcc`, `make`, `flex`, `bison`, `bc`, `libssl-dev`, `libelf-dev`.

## Performance

Cold-boot baseline, no optimization, on a recent x86_64 laptop (`Vm::new` to first guest byte):

| Metric | Time |
|---|---|
| Best of five | 23.6 ms |
| Median | ~38 ms |
| Worst | 91 ms (host scheduling variance) |

Most of the time is in the guest-side kernel boot (cpuid probes, TSC calibration, refined-jiffies init). `Vm::new` is typically <1 ms; ELF load is ~12 ms.

## Scope

In scope:
- Single ephemeral microVM per process.
- Linux x86_64 host and guest.
- virtio-block, virtio-net, virtio-vsock; 8250 UART; KVM-emulated IRQ chip + PIT.

Out of scope, by design:
- Non-Linux guests, aarch64.
- PCI transport, GPU / USB / device passthrough.
- Live migration, memory ballooning, CPU hot-plug, MMDS.
- Multi-tenant density, multi-VM-per-process.
- Daemon mode and REST control plane.

Every new feature must come with a corresponding deletion or a written justification. Anti-goal: feature parity with anything.

## Layout

- `src/kvm/` — `/dev/kvm` ioctl wrappers.
- `src/boot/` — Linux x86_64 boot protocol (ELF load, page tables, GDT, boot_params, MP table).
- `src/devices/` — virtio-mmio transport, virtio-block, virtio-net, virtio-vsock, 8250 UART, CMOS.
- `src/lifecycle/` — `Vm` orchestrator: configuration, state machine, run loop, MMIO/PIO dispatch.
- `src/jailer/` — seccomp + namespaces + cgroups sandbox, separate `ingot-jailer` binary.
- `src/bin/` — the `ingot` CLI.
- `scripts/` — end-to-end test scripts.
- `examples/` — workloads the demo scripts run inside the guest.

## License

Apache 2.0.
