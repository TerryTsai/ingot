//! virtio-net (virtio 1.x §5.1) over MMIO. Queue 0 = RX, queue 1 = TX.
//! Two-queue netdev with optional TAP backing — see `net_io.rs` for the
//! actual frame-flow code. This file is the MMIO-facing surface: device
//! identity, feature negotiation, config space (MAC), notify dispatch.

use super::net_io::{drain_rx, drain_tx};
use super::tap::Tap;
use super::virtio_mmio::VirtioMmio;
use super::{GuestError, Interrupt, MmioDevice};
use crate::memory::Memory;

const DEVICE_ID_NET: u32 = 1;
const VENDOR_ID: u32 = 0x494E_474F;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const NUM_QUEUES: usize = 2;
const QUEUE_TX: u32 = 1;
const CONFIG_BASE: u64 = 0x100;
const MAC_LEN: usize = 6;
const VIRTIO_NET_GSI: u32 = 5;

pub(crate) struct Net {
    pub(super) mmio: VirtioMmio,
    pub(super) mac: [u8; MAC_LEN],
    pub(super) tap: Option<Tap>,
    pub(super) tx_bytes: u64,
    pub(super) tx_packets: u64,
}

impl Net {
    pub fn new(mac: [u8; MAC_LEN], tap: Option<Tap>) -> Self {
        let features = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
        Net { mmio: VirtioMmio::new(DEVICE_ID_NET, VENDOR_ID, features, NUM_QUEUES), mac, tap, tx_bytes: 0, tx_packets: 0 }
    }
}

impl MmioDevice for Net {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE {
            for (i, b) in data.iter_mut().enumerate() {
                let abs = (offset - CONFIG_BASE) as usize + i;
                *b = if abs < MAC_LEN { self.mac[abs] } else { 0 };
            }
            return Ok(());
        }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        data.copy_from_slice(&self.mmio.read_reg(offset).to_le_bytes());
        Ok(())
    }

    fn write(&mut self, offset: u64, data: &[u8], mem: &mut Memory) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE { return Ok(()); }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        let v = u32::from_le_bytes(data.try_into().unwrap());
        if let Some(q) = self.mmio.write_reg(offset, v) {
            if q == QUEUE_TX { drain_tx(self, mem)?; }
        }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_NET_GSI }) } else { None }
    }

    fn poll_io(&mut self, mem: &mut Memory) -> Result<(), GuestError> { drain_rx(self, mem) }
}

#[cfg(test)]
impl Net {
    pub fn tx_stats(&self) -> (u64, u64) { (self.tx_packets, self.tx_bytes) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

    fn new() -> Net { Net::new(MAC, None) }
    fn mem() -> Memory { Memory::new(0x10_0000).unwrap() }

    fn read32(n: &mut Net, offset: u64) -> u32 {
        let mut buf = [0u8; 4]; n.read(offset, &mut buf).unwrap(); u32::from_le_bytes(buf)
    }

    fn write32(n: &mut Net, offset: u64, v: u32, m: &mut Memory) {
        n.write(offset, &v.to_le_bytes(), m).unwrap();
    }

    #[test]
    fn advertises_magic_version_net_device_id() {
        let mut n = new();
        assert_eq!(read32(&mut n, 0x000), 0x7472_6976);
        assert_eq!(read32(&mut n, 0x004), 2);
        assert_eq!(read32(&mut n, 0x008), 1);
    }

    #[test]
    fn mac_appears_in_config_space() {
        let mut n = new();
        let mut buf = [0u8; 6];
        n.read(CONFIG_BASE, &mut buf).unwrap();
        assert_eq!(buf, MAC);
    }

    #[test]
    fn features_include_mac_and_version_1() {
        let mut n = new();
        let mut m = mem();
        let lo = read32(&mut n, 0x010);
        write32(&mut n, 0x014, 1, &mut m);
        let hi = read32(&mut n, 0x010);
        let feats = ((hi as u64) << 32) | (lo as u64);
        assert_ne!(feats & VIRTIO_NET_F_MAC, 0);
        assert_ne!(feats & VIRTIO_F_VERSION_1, 0);
    }

    #[test]
    fn tx_notify_drains_descriptor_chain() {
        let mut n = new();
        let mut m = mem();
        write32(&mut n, 0x030, 1, &mut m);
        write32(&mut n, 0x038, 8, &mut m);
        write32(&mut n, 0x080, 0x1000, &mut m);
        write32(&mut n, 0x090, 0x2000, &mut m);
        write32(&mut n, 0x0a0, 0x3000, &mut m);
        write32(&mut n, 0x044, 1, &mut m);
        m.write(0x1000, &0x4000u64.to_le_bytes()).unwrap();
        m.write(0x1008, &12u32.to_le_bytes()).unwrap();
        m.write(0x100c, &1u16.to_le_bytes()).unwrap();
        m.write(0x100e, &1u16.to_le_bytes()).unwrap();
        m.write(0x1010, &0x5000u64.to_le_bytes()).unwrap();
        m.write(0x1018, &88u32.to_le_bytes()).unwrap();
        m.write(0x101c, &0u16.to_le_bytes()).unwrap();
        m.write(0x101e, &0u16.to_le_bytes()).unwrap();
        m.write(0x2004, &0u16.to_le_bytes()).unwrap();
        m.write(0x2002, &1u16.to_le_bytes()).unwrap();
        write32(&mut n, 0x050, 1, &mut m);
        assert_eq!(n.tx_stats(), (1, 100));
    }
}
