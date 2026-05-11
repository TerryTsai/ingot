//! MMIO bus: maps `(base, size)` regions to `MmioDevice` handles and
//! dispatches reads/writes from the orchestrator's run loop.

use crate::error::HostError;

use super::{GuestError, Interrupt, MmioDevice};

struct Region {
    base: u64,
    size: u64,
    device: Box<dyn MmioDevice>,
}

pub(crate) struct MmioBus {
    regions: Vec<Region>,
}

impl MmioBus {
    pub fn new() -> Self {
        MmioBus { regions: Vec::new() }
    }

    pub fn register(&mut self, base: u64, size: u64, device: Box<dyn MmioDevice>) -> Result<(), HostError> {
        for r in &self.regions {
            if base < r.base + r.size && r.base < base + size {
                return Err(HostError::InvalidConfig("MMIO region overlap"));
            }
        }
        self.regions.push(Region { base, size, device });
        Ok(())
    }

    pub fn read(&mut self, addr: u64, data: &mut [u8]) -> Result<(), GuestError> {
        let region = self.find(addr, data.len())?;
        let offset = addr - region.base;
        region.device.read(offset, data)
    }

    pub fn write(&mut self, addr: u64, data: &[u8]) -> Result<(), GuestError> {
        let region = self.find(addr, data.len())?;
        let offset = addr - region.base;
        region.device.write(offset, data)
    }

    pub fn pending_interrupts(&mut self) -> Vec<(u64, Interrupt)> {
        let mut out = Vec::new();
        for r in &mut self.regions {
            if let Some(intr) = r.device.pending_interrupt() {
                out.push((r.base, intr));
            }
        }
        out
    }

    fn find(&mut self, addr: u64, len: usize) -> Result<&mut Region, GuestError> {
        let len = len as u64;
        for r in &mut self.regions {
            if addr >= r.base && addr.saturating_add(len) <= r.base + r.size {
                return Ok(r);
            }
        }
        Err(GuestError::MmioOutOfRange { addr })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    use super::*;

    struct Counter {
        reads: Arc<AtomicU32>,
        writes: Arc<AtomicU32>,
        last_offset: Arc<AtomicU32>,
    }

    impl Counter {
        fn new() -> (Self, Arc<AtomicU32>, Arc<AtomicU32>, Arc<AtomicU32>) {
            let r = Arc::new(AtomicU32::new(0));
            let w = Arc::new(AtomicU32::new(0));
            let o = Arc::new(AtomicU32::new(0));
            (Counter { reads: r.clone(), writes: w.clone(), last_offset: o.clone() }, r, w, o)
        }
    }

    impl MmioDevice for Counter {
        fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(offset as u32, Ordering::SeqCst);
            for b in data.iter_mut() { *b = 0xAB; }
            Ok(())
        }
        fn write(&mut self, offset: u64, _data: &[u8]) -> Result<(), GuestError> {
            self.writes.fetch_add(1, Ordering::SeqCst);
            self.last_offset.store(offset as u32, Ordering::SeqCst);
            Ok(())
        }
        fn pending_interrupt(&mut self) -> Option<Interrupt> { None }
    }

    #[test]
    fn dispatches_to_registered_device() {
        let mut bus = MmioBus::new();
        let (dev, reads, writes, last) = Counter::new();
        bus.register(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
        bus.write(0xD000_0003, &[0x42]).unwrap();
        let mut buf = [0u8; 1];
        bus.read(0xD000_0005, &mut buf).unwrap();
        assert_eq!(writes.load(Ordering::SeqCst), 1);
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        assert_eq!(last.load(Ordering::SeqCst), 5);
        assert_eq!(buf[0], 0xAB);
    }

    #[test]
    fn rejects_overlap_on_register() {
        let mut bus = MmioBus::new();
        let (a, _, _, _) = Counter::new();
        let (b, _, _, _) = Counter::new();
        bus.register(0xD000_0000, 0x1000, Box::new(a)).unwrap();
        assert!(bus.register(0xD000_0800, 0x1000, Box::new(b)).is_err());
    }

    #[test]
    fn rejects_address_outside_any_region() {
        let mut bus = MmioBus::new();
        let (dev, _, _, _) = Counter::new();
        bus.register(0xD000_0000, 0x1000, Box::new(dev)).unwrap();
        assert!(bus.write(0xD000_2000, &[0x01]).is_err());
    }

    #[test]
    fn dispatches_among_multiple_devices() {
        let mut bus = MmioBus::new();
        let (a, ar, _, _) = Counter::new();
        let (b, br, _, _) = Counter::new();
        bus.register(0xD000_0000, 0x1000, Box::new(a)).unwrap();
        bus.register(0xD000_2000, 0x1000, Box::new(b)).unwrap();
        let mut buf = [0u8; 1];
        bus.read(0xD000_0010, &mut buf).unwrap();
        bus.read(0xD000_2020, &mut buf).unwrap();
        bus.read(0xD000_2024, &mut buf).unwrap();
        assert_eq!(ar.load(Ordering::SeqCst), 1);
        assert_eq!(br.load(Ordering::SeqCst), 2);
    }
}
