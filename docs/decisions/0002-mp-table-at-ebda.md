# 0002: Minimal MP table at EBDA for LAPIC discovery

Date: 2026-05-11
Status: accepted
Phase: 2

## Decision

The boot subsystem writes an Intel MultiProcessor Specification 1.4 configuration table to guest physical address 0x9FC00 (EBDA). It contains a floating pointer, config header, one CPU entry per vCPU, one ISA bus entry, and one IOAPIC entry — ~96 bytes total, all static, no runtime state.

## Why

x86_64 Linux requires a functional LAPIC at runtime (`CONFIG_X86_LOCAL_APIC` is forced `y` by `CONFIG_X86_64`; the noop-APIC fallback can't route timer interrupts and the scheduler stalls). The LAPIC's existence is discovered through one of:

1. ACPI MADT (modern PCs)
2. MP table (legacy spec, still parsed by every x86_64 kernel)
3. Architectural-default fallback (only taken if `boot_cpu_has(X86_FEATURE_APIC)` is true *and* MP/ACPI scanning failed)

We don't emit ACPI tables (out of scope per SPEC.md §2.5). Without MP either, the kernel hits "No local APIC present" because the kernel's discovery code-path takes the `setup_force_cpu_cap(APIC) → cleared by an earlier check` branch. Even forcing `lapic` via cmdline didn't help — the kernel's expectation is that an MP table or MADT advertises the LAPIC.

The MP table is the smallest viable answer: a one-time write of static bytes at boot. It is *not* "rich device emulation" — there is no runtime behavior, no MMIO trap, no IRQ flow. It's the same conceptual category as e820 or `boot_params`: pre-baked boot info the kernel reads once during early init.

## What we picked

- **Address: 0x9FC00 (start of EBDA scan range).** Kernels also scan `[0, 0x400)` and `[0xF0000, 0x100000)`; EBDA is the conventional location and matches Firecracker.
- **Entries**: 1× processor per vCPU (BSP flag on CPU 0), 1× ISA bus, 1× IOAPIC at the architectural default 0xFEC00000.
- **No IRQ override entries.** Default 1:1 IRQ→INTIN mapping is correct for our setup (KVM PIT delivers IRQ 0 to GSI 0, 8250 UART asserts IRQ 4 = GSI 4). The kernel logs "BIOS bug, no explicit IRQ entries, using default mptable" — accepted; it's the documented "I trust your hardware" mode.

## Alternatives rejected

- **ACPI MADT/DSDT**: would require an ACPI subsystem (RSDP, RSDT, MADT, DSDT compiler). Locked behind SPEC.md §2.5.
- **Switch to a kernel patched to register LAPIC unconditionally**: kernel-side bypass; brittle across kernel versions; we'd own a downstream patch forever.
- **Disable LAPIC entirely via `CONFIG_X86_LOCAL_APIC=n`**: forced on by `CONFIG_X86_64=y`. Not selectable.

## Cost paid

- `src/boot/mp_table.rs`, 76 lines of production code (under the 100-line cap, exempt-test).
- One line of orchestration in `vm.rs::boot_kernel` to call `MpTable::write_to`.
- `sregs.apic_base` must be set to `0xFEE0_0900` before `KVM_SET_SREGS`, because the ioctl writes the APIC base MSR and a zero value would disable the in-kernel LAPIC.
