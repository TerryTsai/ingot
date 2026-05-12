//! Address-range bus. Routes guest reads/writes to whichever device
//! claims the address. Two route tables: one for MMIO ranges (u64
//! addresses, devices implementing `MmioDevice`), one for PIO ranges
//! (u16 ports, devices implementing `PioDevice`). The orchestrator's
//! run loop dispatches every MMIO/PIO exit through this bus.

use crate::lifecycle::error::HostError;
use crate::lifecycle::support::memory::Memory;

use super::GuestError;

/// A device-raised IRQ. Devices return one from `pending_interrupt`;
/// the bus collects them and `Vm` reconciles transitions against the
/// in-kernel IRQ chip.
pub(crate) struct Interrupt {
    pub gsi: u32,
}

/// Memory-mapped device contract. Implementors are reachable through the
/// bus's MMIO table at some `(base, size)` range; reads/writes arrive as
/// offsets into that range. `poll_io` runs after each vCPU exit for
/// devices with async host I/O (TAP, vhost call eventfds).
pub(crate) trait MmioDevice: Send {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError>;
    fn write(&mut self, offset: u64, data: &[u8], mem: &mut Memory) -> Result<(), GuestError>;
    fn pending_interrupt(&mut self) -> Option<Interrupt>;
    fn poll_io(&mut self, _mem: &mut Memory) -> Result<(), GuestError> { Ok(()) }
}

/// Port-I/O device contract. Implementors are reachable through the
/// bus's PIO table at some `(base, size)` range; reads/writes arrive
/// as offsets. PIO is x86-only and used here for the legacy 8250 UART
/// and the CMOS RTC shim. No async I/O — PIO devices respond
/// synchronously.
pub(crate) trait PioDevice: Send {
    fn read(&mut self, port_offset: u16, data: &mut [u8]);
    fn write(&mut self, port_offset: u16, data: &[u8]);
    fn pending_interrupt(&mut self) -> Option<Interrupt> { None }
}

struct MmioRegion {
    base: u64,
    size: u64,
    device: Box<dyn MmioDevice>,
}

struct PioRegion {
    base: u16,
    size: u16,
    device: Box<dyn PioDevice>,
}

pub(crate) struct Bus {
    mmio: Vec<MmioRegion>,
    pio: Vec<PioRegion>,
}

impl Bus {
    pub fn new() -> Self {
        Bus { mmio: Vec::new(), pio: Vec::new() }
    }

    // ── MMIO ────────────────────────────────────────────────────────────

    pub fn register_mmio(&mut self, base: u64, size: u64, device: Box<dyn MmioDevice>) -> Result<(), HostError> {
        for r in &self.mmio {
            if base < r.base + r.size && r.base < base + size {
                return Err(HostError::InvalidConfig("MMIO region overlap"));
            }
        }
        self.mmio.push(MmioRegion { base, size, device });
        Ok(())
    }

    pub fn read_mmio(&mut self, addr: u64, data: &mut [u8]) -> Result<(), GuestError> {
        let region = self.find_mmio(addr, data.len())?;
        let offset = addr - region.base;
        region.device.read(offset, data)
    }

    pub fn write_mmio(&mut self, addr: u64, data: &[u8], mem: &mut Memory) -> Result<(), GuestError> {
        let region = self.find_mmio(addr, data.len())?;
        let offset = addr - region.base;
        region.device.write(offset, data, mem)
    }

    // ── PIO ─────────────────────────────────────────────────────────────

    pub fn register_pio(&mut self, base: u16, size: u16, device: Box<dyn PioDevice>) -> Result<(), HostError> {
        for r in &self.pio {
            if base < r.base + r.size && r.base < base + size {
                return Err(HostError::InvalidConfig("PIO region overlap"));
            }
        }
        self.pio.push(PioRegion { base, size, device });
        Ok(())
    }

    /// Read from a PIO port. Unrouted ports return all-0xFF, matching the
    /// PC convention for absent hardware.
    pub fn read_pio(&mut self, port: u16, data: &mut [u8]) {
        if let Some(r) = self.find_pio(port, data.len() as u16) {
            r.device.read(port - r.base, data);
        } else {
            for b in data.iter_mut() { *b = 0xFF; }
        }
    }

    /// Write to a PIO port. Unrouted ports drop the write silently.
    pub fn write_pio(&mut self, port: u16, data: &[u8]) {
        if let Some(r) = self.find_pio(port, data.len() as u16) {
            r.device.write(port - r.base, data);
        }
    }

    // ── interrupts + async polling ──────────────────────────────────────

    pub fn pending_interrupts(&mut self) -> Vec<Interrupt> {
        let mut out = Vec::new();
        for r in &mut self.mmio {
            if let Some(intr) = r.device.pending_interrupt() { out.push(intr); }
        }
        for r in &mut self.pio {
            if let Some(intr) = r.device.pending_interrupt() { out.push(intr); }
        }
        out
    }

    pub fn poll_io(&mut self, mem: &mut Memory) -> Result<(), GuestError> {
        for r in &mut self.mmio {
            r.device.poll_io(mem)?;
        }
        Ok(())
    }

    fn find_mmio(&mut self, addr: u64, len: usize) -> Result<&mut MmioRegion, GuestError> {
        let len = len as u64;
        for r in &mut self.mmio {
            if addr >= r.base && addr.saturating_add(len) <= r.base + r.size {
                return Ok(r);
            }
        }
        Err(GuestError::MmioOutOfRange { addr })
    }

    fn find_pio(&mut self, port: u16, len: u16) -> Option<&mut PioRegion> {
        self.pio.iter_mut().find(|r| {
            port >= r.base && port.saturating_add(len) <= r.base + r.size
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use super::*;

    struct MmioCounter {
        reads: Arc<AtomicU32>,
        writes: Arc<AtomicU32>,
        last_offset: Arc<AtomicU32>,
    }

    impl MmioCounter {
        fn new() -> (Self, Arc<AtomicU32>, Arc<AtomicU32>, Arc<AtomicU32>) {
            let r = Arc::new(AtomicU32::new(0));
            let w = Arc::new(AtomicU32::new(0));
            let o = Arc::new(AtomicU32::new(0));
            (MmioCounter { reads: r.clone(), writes: w.clone(), last_offset: o.clone() }, r, w, o)
        }
    }

    impl MmioDevice for MmioCounter {
        fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(offset as u32, Ordering::SeqCst);
            for b in data.iter_mut() { *b = 0xAB; }
            Ok(())
        }
        fn write(&mut self, offset: u64, _data: &[u8], _mem: &mut Memory) -> Result<(), GuestError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(offset as u32, Ordering::SeqCst);
            Ok(())
        }
        fn pending_interrupt(&mut self) -> Option<Interrupt> { None }
    }

    struct PioCounter {
        reads: Arc<AtomicU32>,
        writes: Arc<AtomicU32>,
        last_offset: Arc<AtomicU32>,
    }

    impl PioCounter {
        fn new() -> (Self, Arc<AtomicU32>, Arc<AtomicU32>, Arc<AtomicU32>) {
            let r = Arc::new(AtomicU32::new(0));
            let w = Arc::new(AtomicU32::new(0));
            let o = Arc::new(AtomicU32::new(0));
            (PioCounter { reads: r.clone(), writes: w.clone(), last_offset: o.clone() }, r, w, o)
        }
    }

    impl PioDevice for PioCounter {
        fn read(&mut self, port_offset: u16, data: &mut [u8]) {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(port_offset as u32, Ordering::SeqCst);
            for b in data.iter_mut() { *b = 0x77; }
        }
        fn write(&mut self, port_offset: u16, _data: &[u8]) {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(port_offset as u32, Ordering::SeqCst);
        }
    }

    fn mem() -> Memory { Memory::new(0x10_0000).expect("mem") }

    #[test]
    fn mmio_dispatches_to_registered_device() {
        let mut bus = Bus::new();
        let mut m = mem();
        let (dev, reads, writes, last) = MmioCounter::new();
        bus.register_mmio(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
        bus.write_mmio(0xD000_0003, &[0x42], &mut m).unwrap();
        let mut buf = [0u8; 1];
        bus.read_mmio(0xD000_0005, &mut buf).unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(last.load(Ordering::SeqCst), 5);
        assert_eq!(buf[0], 0xAB);
    }

    #[test]
    fn mmio_rejects_overlap_on_register() {
        let mut bus = Bus::new();
        let (a, _, _, _) = MmioCounter::new();
        let (b, _, _, _) = MmioCounter::new();
        bus.register_mmio(0xD000_0000, 0x1000, Box::new(a)).unwrap();
        assert!(bus.register_mmio(0xD000_0800, 0x1000, Box::new(b)).is_err());
    }

    #[test]
    fn mmio_rejects_address_outside_any_region() {
        let mut bus = Bus::new();
        let mut m = mem();
        let (dev, _, _, _) = MmioCounter::new();
        bus.register_mmio(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
        assert!(bus.write_mmio(0xD000_2000, &[0x01], &mut m).is_err());
    }

    #[test]
    fn pio_dispatches_to_registered_device() {
        let mut bus = Bus::new();
        let (dev, reads, writes, last) = PioCounter::new();
        bus.register_pio(0x3F8, 8, Box::new(dev)).unwrap();
        bus.write_pio(0x3F9, &[0x42]);
        let mut buf = [0u8; 1];
        bus.read_pio(0x3FA, &mut buf);
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(last.load(Ordering::SeqCst), 2);
        assert_eq!(buf[0], 0x77);
    }

    #[test]
    fn pio_returns_ff_for_unrouted_port() {
        let mut bus = Bus::new();
        let mut buf = [0u8; 1];
        bus.read_pio(0x80, &mut buf);
        assert_eq!(buf[0], 0xFF);
        bus.write_pio(0x80, &[0x00]); // dropped, no panic
    }

    #[test]
    fn pio_rejects_overlap_on_register() {
        let mut bus = Bus::new();
        let (a, _, _, _) = PioCounter::new();
        let (b, _, _, _) = PioCounter::new();
        bus.register_pio(0x3F8, 8, Box::new(a)).unwrap();
        assert!(bus.register_pio(0x3FC, 4, Box::new(b)).is_err());
    }

    // ── property tests (SPEC §3.3: hostile-guest addresses) ────────────
    //
    // The contract: routing decisions on any (addr, len) pair never panic.
    // Out-of-range MMIO returns MmioOutOfRange; out-of-range PIO returns
    // all-0xFF for reads and silently drops writes.

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig { cases: 200, .. ProptestConfig::default() })]

        /// MMIO read at any address never panics.
        #[test]
        fn mmio_read_terminates_on_any_address(addr in any::<u64>(), len in 1usize..9) {
            let mut bus = Bus::new();
            let (dev, _, _, _) = MmioCounter::new();
            bus.register_mmio(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
            let mut buf = [0u8; 8];
            let _ = bus.read_mmio(addr, &mut buf[..len]);
        }

        /// MMIO write at any address never panics.
        #[test]
        fn mmio_write_terminates_on_any_address(addr in any::<u64>(), data in prop::collection::vec(any::<u8>(), 1..9)) {
            let mut bus = Bus::new();
            let mut m = mem();
            let (dev, _, _, _) = MmioCounter::new();
            bus.register_mmio(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
            let _ = bus.write_mmio(addr, &data, &mut m);
        }

        /// PIO read at any port returns valid data without panicking.
        /// Unrouted ports return 0xFF.
        #[test]
        fn pio_read_returns_ff_outside_any_region(port in any::<u16>(), len in 1usize..5) {
            let mut bus = Bus::new();
            let (dev, _, _, _) = PioCounter::new();
            bus.register_pio(0x3F8, 8, Box::new(dev)).unwrap();
            let mut buf = [0u8; 4];
            bus.read_pio(port, &mut buf[..len]);
            // If port is outside [0x3F8, 0x3FF], expect 0xFF for all bytes.
            if port < 0x3F8 || port.saturating_add(len as u16) > 0x400 {
                for b in &buf[..len] { prop_assert_eq!(*b, 0xFF); }
            }
        }

        /// PIO write at any port either dispatches or silently drops.
        /// Never panics.
        #[test]
        fn pio_write_terminates_on_any_port(port in any::<u16>(), data in prop::collection::vec(any::<u8>(), 1..5)) {
            let mut bus = Bus::new();
            let (dev, _, _, _) = PioCounter::new();
            bus.register_pio(0x3F8, 8, Box::new(dev)).unwrap();
            bus.write_pio(port, &data);
        }
    }
}
