//! virtio-net (virtio 1.x §5.1) over MMIO. Two virtqueues: 0 = RX
//! (device fills with received frames), 1 = TX (driver fills with
//! frames to transmit). Phase 4 milestone A: device is enumerable and
//! feature negotiation works; the data path is wired in a later pass.

use super::virtio_mmio::VirtioMmio;
use super::{GuestError, Interrupt, MmioDevice};

const DEVICE_ID_NET: u32 = 1;
const VENDOR_ID: u32 = 0x494E_474F;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const NUM_QUEUES: usize = 2;
const CONFIG_BASE: u64 = 0x100;
const MAC_LEN: usize = 6;

pub(crate) struct Net {
    mmio: VirtioMmio,
    mac: [u8; MAC_LEN],
}

impl Net {
    pub fn new(mac: [u8; MAC_LEN]) -> Self {
        let features = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
        Net { mmio: VirtioMmio::new(DEVICE_ID_NET, VENDOR_ID, features, NUM_QUEUES), mac }
    }

    fn read_config(&self, off: u64, data: &mut [u8]) {
        for (i, b) in data.iter_mut().enumerate() {
            let abs = off as usize + i;
            *b = if abs < MAC_LEN { self.mac[abs] } else { 0 };
        }
    }
}

impl MmioDevice for Net {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE {
            self.read_config(offset - CONFIG_BASE, data);
            return Ok(());
        }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        let v = self.mmio.read_reg(offset);
        data.copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn write(&mut self, offset: u64, data: &[u8]) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE { return Ok(()); }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        let v = u32::from_le_bytes(data.try_into().unwrap());
        let _notify = self.mmio.write_reg(offset, v);
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

    fn new() -> Net { Net::new(MAC) }

    fn read32(n: &mut Net, offset: u64) -> u32 {
        let mut buf = [0u8; 4];
        n.read(offset, &mut buf).unwrap();
        u32::from_le_bytes(buf)
    }

    fn write32(n: &mut Net, offset: u64, v: u32) {
        n.write(offset, &v.to_le_bytes()).unwrap();
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
        let lo = read32(&mut n, 0x010);
        write32(&mut n, 0x014, 1);
        let hi = read32(&mut n, 0x010);
        let feats = ((hi as u64) << 32) | (lo as u64);
        assert_ne!(feats & VIRTIO_NET_F_MAC, 0);
        assert_ne!(feats & VIRTIO_F_VERSION_1, 0);
    }

    #[test]
    fn status_round_trips() {
        let mut n = new();
        write32(&mut n, 0x070, 0x07);
        assert_eq!(read32(&mut n, 0x070), 0x07);
    }
}
