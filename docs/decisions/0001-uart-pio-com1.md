# 0001: 8250 UART on PIO COM1, not MMIO

Date: 2026-05-11
Status: accepted
Phase: 2

## Decision

The 8250 UART is exposed at PIO 0x3F8–0x3FF (COM1), not at MMIO. virtio devices remain MMIO-only. SPEC.md §3.1.3 is amended.

## Why

The kernel detects an MMIO-only 8250 console only via a DT/ACPI table or by patching `serial8250_register_8250_port` into kernel C code. The microvm convention on x86_64 (Firecracker, crosvm's legacy mode) is to put the 8250 at PIO COM1, where Linux's 8250 driver auto-detects it from a static probe table and registers it as `ttyS0` with no kernel-side plumbing. With `console=ttyS0,115200`, `/dev/console` is routed to it and user-space writes work.

`console=uart,mmio,0xD000_0000` (our prior approach) only registers an earlycon. Earlycons are write-only polling consoles for boot-time `printk`. They do not back `/dev/console`, so user-space init can't open a working stdio — which manifested as kernel boot succeeding but init exiting immediately on EOF.

## Alternatives rejected

- **Devicetree blob via `setup_data` SETUP_DTB**: requires `CONFIG_OF=y` + `CONFIG_OF_PLATFORM=y` + `CONFIG_SERIAL_OF_PLATFORM=y`, and we'd hand-roll FDT serialization (~200 lines). Locks the kernel into a specific config. PIO COM1 works on any stock x86_64 kernel.
- **ACPI MADT/DSDT with an 8250 entry**: scope-creep into ACPI table emulation, which §2.5 puts permanently out of scope.
- **Kernel patch to register MMIO console at our address**: requires maintaining a downstream patch and renders the kernel image non-stock.

## Cost paid

- §3.1.3 amended to permit PIO transport for the 8250 specifically. virtio is still MMIO-only.
- The `MmioDevice` trait no longer covers the UART; UART exposes plain `read(reg)` / `write(reg, val)` methods. Lifecycle dispatches PIO inline.
- IRQ injection wired through `KVM_IRQ_LINE` after each vCPU exit. Without IRQs, kernel printk works (polling path) but user-space writes via the tty layer hang waiting for the THRE interrupt.
- One IRQ-routing concession: the 8250 fires GSI 4 (legacy COM1). The MP table's default 1:1 IRQ→INTIN mapping handles this; no IRQ override entries needed.

## What stays out of scope

- Other legacy PIO devices (keyboard 8042, PIT directly, IDE) — KVM's in-kernel PIT handles timer ports; everything else returns 0xFF / drops writes.
- DMA, ISA bus arbitration, real interrupt sharing — single device on COM1, no contention.
