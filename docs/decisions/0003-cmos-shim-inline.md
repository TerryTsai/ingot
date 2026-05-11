# 0003: CMOS RTC shim, inline in `vm.rs`, not a device

Date: 2026-05-11
Status: accepted
Phase: 2

## Decision

The CMOS RTC at PIO 0x70 (index) / 0x71 (data) is shimmed inline in `vm.rs::handle_pio_in` and `handle_pio_out`. It is not a `Device`, not in `src/devices/`, and does not implement `MmioDevice`.

Behavior:
- Writes to 0x70 latch a 7-bit register index into `Vm.cmos_index`.
- Reads from 0x71 return a small lookup table: status A = 0x26 (RTC running, UIP=0), status B = 0x06 (24h, binary mode), status D = 0x80 (RAM valid), everything else (time fields) = 0.
- Writes to 0x71 are dropped.

Total: ~10 lines of dispatch + one byte of state.

## Why

`arch/x86/kernel/rtc.c::read_persistent_clock` is unconditionally compiled into every x86_64 kernel and is called from `timekeeping_init`. It reads CMOS to set initial wallclock time. Its inner loop polls register 0x0A's bit 7 (Update In Progress) until it clears.

Our prior default — return 0xFF for all unhandled PIO reads — sets bit 7 permanently → kernel busy-loops forever. Switching the default to 0x00 instead breaks PIT-based TSC calibration (which uses port 0x61's refresh-toggle bit and expects 0xFF → 0x00 transitions). So the dispatch must be port-specific.

## Why not a device

The shim is two reads (status A, status D) and one bit of state (which register was just selected). Creating a `Cmos` struct in `src/devices/` with `read`/`write` methods would be 30 lines of identical machinery for behavior that fits in 10 lines inline. Per SPEC.md §5.1 (functions over types), the inline form is the right shape until the shim grows behavior that justifies encapsulation (e.g. real wallclock or alarm interrupts).

## Cost paid

- One field on `Vm` (`cmos_index: u8`).
- Two `match` arms in the PIO dispatcher.
- One free function `cmos_read(index: u8) -> u8` at the bottom of `vm.rs`.

## When to revisit

Promote to a primitive in `src/devices/` if any of:
- The shim needs real wallclock time (requires host clock plumbing).
- An alarm/IRQ on CMOS is needed (would add `pending_irq` state).
- A second PIO device is added — at that point a `PioBus` parallel to `MmioBus` may be warranted, and CMOS goes on it.
