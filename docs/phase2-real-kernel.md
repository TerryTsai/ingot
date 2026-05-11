# Phase 2 real-kernel boot — journal

Status as of 2026-05-11: Phase 2 done-criterion met with the Firecracker
quickstart kernel + busybox initramfs. Custom microvm-tuned kernel build
is in progress as the polished final state.

## Done-criterion (SPEC.md §4)

> `cargo run --bin ingot -- run --kernel vmlinux --initramfs init.cpio`
> boots to an interactive shell over serial within reasonable time.

Met with `/tmp/fc-vmlinux.bin` (Firecracker quickstart, 4.14.174) +
`/tmp/initramfs.cpio.gz` (busybox-static 1.35.0). Banner + `/ #` prompt
appear within ~6s wallclock. Piped commands execute and produce output;
`uname -a` reports `Linux (none) 4.14.174 #2 SMP ... x86_64 GNU/Linux`.

## What was added beyond the kernel boot

Four pieces of VMM-side work, each documented:

1. **MP table at EBDA (0x9FC00).** `src/boot/mp_table.rs`. Without it,
   x86_64 kernels can't discover the LAPIC (ACPI is permanently out of
   scope) and fall back to noop APIC → scheduler stalls. See
   [docs/decisions/0002-mp-table-at-ebda.md](decisions/0002-mp-table-at-ebda.md).

2. **`sregs.apic_base` set before `KVM_SET_SREGS`.** `0xFEE0_0900` =
   xAPIC enabled at default address + BSP flag. The ioctl writes the
   APIC base MSR; leaving it 0 disables the in-kernel LAPIC so even
   LAPIC discovery via MP table doesn't help.

3. **CMOS RTC shim inline in `vm.rs`.** `read_persistent_clock` busy-loops
   on CMOS Status A bit 7 (Update In Progress). Returning 0xFF for all
   PIO never clears UIP; the shim returns 0x26 for Status A so the
   kernel proceeds. See [docs/decisions/0003-cmos-shim-inline.md](decisions/0003-cmos-shim-inline.md).

4. **8250 UART moved from MMIO to PIO COM1 + IRQ injection.** MMIO 8250
   on x86_64 requires DT/ACPI plumbing to register as a real console
   (vs. earlycon). PIO 0x3F8 is auto-detected as `ttyS0` by Linux's
   8250 driver. Without IRQs the kernel printk path works (polling) but
   user-space writes via tty hang waiting for THRE. `KVM_IRQ_LINE`
   wired to UART's `pending_irq()` after each vCPU exit. See
   [docs/decisions/0001-uart-pio-com1.md](decisions/0001-uart-pio-com1.md).

## Custom kernel build (in progress)

Target: minimally viable Linux 6.6 LTS kernel for compute workloads in
high-level languages (Python, Node, etc.) with networking config bits
enabled (virtio-net driver) so Phase 4 can wire the device side without
a kernel rebuild.

Build path:

```sh
cd /tmp/kbuild/linux-6.6.30
make tinyconfig
./scripts/kconfig/merge_config.sh -m .config /tmp/ingot.config-fragment
make olddefconfig
make -j$(nproc) vmlinux
```

The config fragment turns on (relative to tinyconfig):
- `CONFIG_X86_64`, `CONFIG_SMP=n`, `CONFIG_HYPERVISOR_GUEST`, `CONFIG_KVM_GUEST`
- 8250 serial console (`CONFIG_SERIAL_8250_CONSOLE`)
- Initramfs + ELF + script binfmt
- tmpfs / procfs / sysfs / devtmpfs (no other filesystems)
- `FUTEX`, `EPOLL`, `EVENTFD`, `TIMERFD`, `SIGNALFD`, `INOTIFY_USER` (runtime needs)
- `NET`, `INET`, `IPV6`, `VIRTIO`, `VIRTIO_MMIO`, `VIRTIO_NET`
- `BLK_DEV_INITRD`, `RD_GZIP`

Off: modules, PCI, ACPI, USB, sound, framebuffer, namespaces, cgroups,
audit, KEYS, KALLSYMS, KASLR, every filesystem except the four above,
every device driver except 8250 and the virtio family.

## Networking — partial

The custom kernel will have the virtio-net driver. The host-side
virtio-net MMIO device (`src/devices/net.rs`) is currently a stub —
implementing it is Phase 4 scope (~2 weeks per SPEC.md §4). Once that
exists, the same kernel will see a working NIC with no rebuild.
