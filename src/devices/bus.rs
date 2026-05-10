//! MMIO bus: maps `(base, size)` regions to `MmioDevice` handles and
//! dispatches reads/writes from the lifecycle subsystem's run loop.

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

    pub fn register(
        &mut self,
        base: u64,
        size: u64,
        device: Box<dyn MmioDevice>,
    ) -> Result<(), HostError> {
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
