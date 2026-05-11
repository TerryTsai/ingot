//! virtio-net (virtio 1.x §5.1) over MMIO. Two virtqueues:
//! 0 = RX (device fills with received frames), 1 = TX (driver fills
//! with frames to transmit). Phase 4 milestone B: TX queue is drained
//! on notify, descriptor chains parsed, bytes counted. Actual TAP I/O
//! and RX path are the next pass.

use super::virtio_mmio::VirtioMmio;
use super::virtio_queue::DESC_F_NEXT;
use super::{GuestError, Interrupt, MmioDevice};
use crate::memory::Memory;

const DEVICE_ID_NET: u32 = 1;
const VENDOR_ID: u32 = 0x494E_474F;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_NET_F_MAC: u64 = 1 << 5;
const NUM_QUEUES: usize = 2;
#[allow(dead_code)]
const QUEUE_RX: u32 = 0;
const QUEUE_TX: u32 = 1;
const CONFIG_BASE: u64 = 0x100;
const MAC_LEN: usize = 6;
const VIRTIO_NET_GSI: u32 = 5;
const INT_USED_RING: u32 = 1;

pub(crate) struct Net {
    mmio: VirtioMmio,
    mac: [u8; MAC_LEN],
    tx_bytes: u64,
    tx_packets: u64,
}

impl Net {
    pub fn new(mac: [u8; MAC_LEN]) -> Self {
        let features = VIRTIO_F_VERSION_1 | VIRTIO_NET_F_MAC;
        Net {
            mmio: VirtioMmio::new(DEVICE_ID_NET, VENDOR_ID, features, NUM_QUEUES),
            mac, tx_bytes: 0, tx_packets: 0,
        }
    }

    pub fn tx_stats(&self) -> (u64, u64) { (self.tx_packets, self.tx_bytes) }

    fn read_config(&self, off: u64, data: &mut [u8]) {
        for (i, b) in data.iter_mut().enumerate() {
            let abs = off as usize + i;
            *b = if abs < MAC_LEN { self.mac[abs] } else { 0 };
        }
    }

    fn drain_tx(&mut self, mem: &mut Memory) -> Result<(), GuestError> {
        let q = &mut self.mmio.queues[QUEUE_TX as usize];
        let cap = q.num as usize;
        let mut serviced = 0u32;
        while let Some(head) = q.pop_avail(mem)? {
            let mut idx = head;
            let mut total: u32 = 0;
            for _ in 0..cap {
                let d = q.read_desc(mem, idx)?;
                total = total.saturating_add(d.len);
                if d.flags & DESC_F_NEXT == 0 { break; }
                idx = d.next;
            }
            q.push_used(mem, head, total)?;
            self.tx_packets += 1;
            self.tx_bytes += total as u64;
            serviced += 1;
        }
        if serviced > 0 {
            self.mmio.interrupt_status |= INT_USED_RING;
        }
        Ok(())
    }
}

impl MmioDevice for Net {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE {
            self.read_config(offset - CONFIG_BASE, data);
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
            if q == QUEUE_TX { self.drain_tx(mem)?; }
        }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_NET_GSI }) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: [u8; 6] = [0x52, 0x54, 0x00, 0x12, 0x34, 0x56];

    fn new() -> Net { Net::new(MAC) }
    fn mem() -> Memory { Memory::new(0x10_0000).unwrap() }

    fn read32(n: &mut Net, offset: u64) -> u32 {
        let mut buf = [0u8; 4];
        n.read(offset, &mut buf).unwrap();
        u32::from_le_bytes(buf)
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

    /// End-to-end virtqueue test: configure TX queue, plant a 100-byte
    /// descriptor chain (vnet_hdr + 88-byte payload), advance avail.idx,
    /// fire notify, and verify drain accounted both descriptors.
    #[test]
    fn tx_notify_drains_descriptor_chain() {
        let mut n = new();
        let mut m = mem();
        // QueueSel = TX (1), QueueNum = 8, QueueReady = 1
        write32(&mut n, 0x030, 1, &mut m);
        write32(&mut n, 0x038, 8, &mut m);
        write32(&mut n, 0x080, 0x1000, &mut m); // desc addr lo
        write32(&mut n, 0x090, 0x2000, &mut m); // avail addr lo
        write32(&mut n, 0x0a0, 0x3000, &mut m); // used addr lo
        write32(&mut n, 0x044, 1, &mut m); // queue ready
        // Plant descriptor 0: 12-byte vnet_hdr at 0x4000, NEXT → desc 1
        m.write(0x1000, &0x4000u64.to_le_bytes()).unwrap();
        m.write(0x1008, &12u32.to_le_bytes()).unwrap();
        m.write(0x100c, &1u16.to_le_bytes()).unwrap(); // flags=NEXT
        m.write(0x100e, &1u16.to_le_bytes()).unwrap(); // next=1
        // Descriptor 1: 88-byte payload at 0x5000, no NEXT
        m.write(0x1010, &0x5000u64.to_le_bytes()).unwrap();
        m.write(0x1018, &88u32.to_le_bytes()).unwrap();
        m.write(0x101c, &0u16.to_le_bytes()).unwrap();
        m.write(0x101e, &0u16.to_le_bytes()).unwrap();
        // avail.ring[0] = 0 (head index), avail.idx = 1
        m.write(0x2004, &0u16.to_le_bytes()).unwrap();
        m.write(0x2002, &1u16.to_le_bytes()).unwrap();
        // Notify TX
        write32(&mut n, 0x050, 1, &mut m);
        assert_eq!(n.tx_stats(), (1, 100));
    }
}
