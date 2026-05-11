# Phase 2 real-kernel boot — running journal

What we know, what we tried, and what the next pass must do to satisfy SPEC.md §4 Phase 2's done criterion:

> `cargo run --bin ingot -- run --kernel vmlinux --initramfs init.cpio` boots to an interactive shell over serial within reasonable time.

## What works (verified end-to-end on KVM)

- Long-mode VM entry (commit `c14a02e` — TR/LDT fix).
- Identity-mapped 1 GiB pages over 4 GiB; MMIO bus at 0xD000_0000 is reachable.
- ELF parser handles the standard `vmlinux` shape (Debian's distro kernel + Firecracker's microvm kernel both parse and load).
- The full boot path: validate → KVM → memory → vCPU → bus → load kernel → write cmdline → boot_params → page tables → GDT → prime_vcpu → run loop. The "Hi" synth test captures stdout end-to-end (commit `c14a02e`).
- IRQCHIP (in-kernel APIC/IOAPIC/LAPIC) with `KVM_SET_TSS_ADDR` precursor (commit `9b91cf8`).
- 61 tests passing under real KVM in ~0.17 s.

## What doesn't (yet)

Both **Debian's `/boot/vmlinuz-6.1.0-23-amd64`** (extracted via `scripts/extract-vmlinux`) and **Firecracker's quickstart `vmlinux.bin`** (from `s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin`) triple-fault immediately:

```
$ cargo run --bin ingot -- run --kernel /tmp/fc-vmlinux.bin --memory 256
ingot: exit=GuestFault(TripleFault)
```

Zero MMIO, zero PIO. The kernel runs some instructions (otherwise we'd see `FAIL_ENTRY`, not `SHUTDOWN`) but crashes before touching the UART. Both kernels' entry points decode cleanly to standard `startup_64` → `call verify_cpu` → `call __startup_64`. The crash is somewhere in that chain.

## The diagnosis short-list

In rough order of likelihood:

1. **`KVM_SET_CPUID2` is not called.** Without it, KVM advertises host CPUID by default; this works for some kernels but real Linux x86_64 boot expects specific feature bits in a specific shape. Most VMMs (Firecracker, crosvm, qemu) explicitly construct a CPUID table via `KVM_GET_SUPPORTED_CPUID` then `KVM_SET_CPUID2`. Concretely: we'd add `KvmFd::supported_cpuid()` + `VcpuFd::set_cpuid2(...)` primitives, call them between `create_vcpu` and `set_sregs`.
2. **Initial MSR state is wrong.** Things like `MSR_IA32_MISC_ENABLE`, `MSR_KVM_SYSTEM_TIME_NEW` may need explicit values. The kernel reads them in `verify_cpu` / `__startup_64`.
3. **No `KVM_SET_MP_STATE`.** Default `mp_state` for BSP under IRQCHIP should be `RUNNABLE`, but worth verifying. If it's `INIT_RECEIVED`, the vCPU sits idle.
4. **Specific cmdline expectation.** Modern kernels require some flags (`console=`, `earlycon=`, possibly `no_timer_check`) to make progress. Our default cmdline mentions earlycon but may not match what the kernel expects.

Order of attack for the next pass: **(1) first** — CPUID is the most commonly-required missing piece. If kernel still triple-faults after CPUID, instrument `Vm::run` to capture `RIP` at crash via `KVM_GET_REGS` and look up the symbol in the kernel's symbol table.

## What we can't do this pass

Building a fresh kernel from source needs build deps we don't have:

```
flex, bison, bc, libelf-dev   # missing; require `sudo apt install`
gcc, make, libssl-dev          # present
```

Sudo requires a password. To proceed via a custom kernel build (the most reliable path), one-time setup:

```sh
sudo apt install flex bison bc libelf-dev
```

After that, the build path is:

```sh
cd /tmp
curl -O https://cdn.kernel.org/pub/linux/kernel/v6.x/linux-6.6.tar.xz
tar xf linux-6.6.tar.xz
cd linux-6.6
# Use Firecracker's published microvm config as a starting point:
curl -O https://raw.githubusercontent.com/firecracker-microvm/firecracker/main/resources/guest_configs/microvm-kernel-ci-x86_64-6.1.config
mv microvm-kernel-ci-x86_64-6.1.config .config
make olddefconfig
make -j$(nproc) vmlinux
# Result: ./vmlinux is an uncompressed ELF, ~10 MB, microvm-tuned.
```

That kernel may boot under our current VMM if the boot path works for a minimal config. If it triple-faults too, the diagnosis above applies.

## Initramfs

Not needed yet; Phase 2 done-criterion needs an interactive shell which requires:

```sh
# Statically-linked busybox + /init script + cpio archive
# Roughly:
mkdir -p /tmp/initramfs/{bin,sbin,etc,proc,sys}
cp $(which busybox) /tmp/initramfs/bin/
cat > /tmp/initramfs/init <<'EOF'
#!/bin/busybox sh
mount -t proc proc /proc
mount -t sysfs sys /sys
exec /bin/busybox sh
EOF
chmod +x /tmp/initramfs/init
cd /tmp/initramfs && find . | cpio -o -H newc | gzip > /tmp/initramfs.cpio.gz
```

## Concrete next-pass actions

In order:

1. Add `KVM_SET_CPUID2` primitive in `kvm/vcpu_fd.rs` (and `KVM_GET_SUPPORTED_CPUID` system-fd ioctl in `kvm/kvm_fd.rs`). Call them in `Vm::new` after `create_vcpu`. ~50 lines.
2. Retry boot. Capture `RIP` at crash via `vcpu.get_regs()` if it still fails; cross-reference kernel symbols.
3. If still failing, add `KVM_SET_MP_STATE` (`RUNNABLE`) and any required MSRs surfaced by the RIP check.
4. Once kernel prints anything, build a busybox initramfs and aim for the shell prompt.

Each is a small, scope-clean addition. Iteration count: hard to predict; likely 3-6 commits.
