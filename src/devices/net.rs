//! virtio-net (virtio 1.x §5.1) over MMIO. Queue 0 = RX, queue 1 = TX.
//! Optional TAP backing — `drain_tx` walks the TX virtqueue's available
//! chains, strips the leading 12-byte vnet_hdr, and writes the L2 frame
//! to the TAP fd. `drain_rx` does the inverse on non-blocking TAP reads:
//! prefix a zeroed vnet_hdr, scatter into the next free RX chain.

use super::tap::Tap;
use super::virtio_mmio::{VirtioMmio, CONFIG_BASE, INT_USED_RING, VENDOR_ID, VIRTIO_F_VERSION_1};
use super::{GuestError, Interrupt, MmioDevice};
use crate::memory::Memory;

const DEVICE_ID_NET: u32 = 1;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const VIRTIO_NET_GSI: u32 = 5;
const NUM_QUEUES: usize = 2;
const QUEUE_RX: usize = 0;
const QUEUE_TX: usize = 1;
const MAC_LEN: usize = 6;
const VNET_HDR_LEN: usize = 12;
const FRAME_MAX: usize = 2048;

pub(crate) struct Net {
    mmio: VirtioMmio,
    mac: [u8; MAC_LEN],
    tap: Option<Tap>,
    tx_bytes: u64,
    tx_packets: u64,
}

impl Net {
    pub fn new(mac: [u8; MAC_LEN], tap: Option<Tap>) -> Self {
        let features = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
        Net {
            mmio: VirtioMmio::new(DEVICE_ID_NET, VENDOR_ID, features, NUM_QUEUES),
            mac, tap, tx_bytes: 0, tx_packets: 0,
        }
    }

    fn drain_tx(&mut self, mem: &mut Memory) -> Result<(), GuestError> {
        let q = &mut self.mmio.queues[QUEUE_TX];
        let mut frame = Vec::with_capacity(FRAME_MAX);
        let mut serviced = 0u32;
        while let Some(head) = q.pop_avail(mem)? {
            frame.clear();
            q.read_chain(mem, head, &mut frame)?;
            if let Some(tap) = &self.tap {
                if frame.len() > VNET_HDR_LEN { let _ = tap.write(&frame[VNET_HDR_LEN..]); }
            }
            let total = frame.len() as u32;
            q.push_used(mem, head, total)?;
            self.tx_packets += 1;
            self.tx_bytes += total as u64;
            serviced += 1;
        }
        if serviced > 0 { self.mmio.interrupt_status |= INT_USED_RING; }
        Ok(())
    }

    fn drain_rx(&mut self, mem: &mut Memory) -> Result<(), GuestError> {
        let tap = match &self.tap { Some(t) => t, None => return Ok(()) };
        let mut buf = [0u8; FRAME_MAX];
        let mut serviced = 0u32;
        loop {
            let n_read = tap.read(&mut buf).map_err(|_| GuestError::MalformedDescriptor)?;
            if n_read == 0 { break; }
            let q = &mut self.mmio.queues[QUEUE_RX];
            let head = match q.pop_avail(mem)? { Some(h) => h, None => break };
            let mut framed = Vec::with_capacity(VNET_HDR_LEN + n_read);
            framed.extend_from_slice(&[0u8; VNET_HDR_LEN]);
            framed.extend_from_slice(&buf[..n_read]);
            let w = q.write_chain(mem, head, &framed)?;
            q.push_used(mem, head, w)?;
            serviced += 1;
        }
        if serviced > 0 { self.mmio.interrupt_status |= INT_USED_RING; }
        Ok(())
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
            if q as usize == QUEUE_TX { self.drain_tx(mem)?; }
        }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_NET_GSI }) } else { None }
    }

    fn poll_io(&mut self, mem: &mut Memory) -> Result<(), GuestError> { self.drain_rx(mem) }
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
