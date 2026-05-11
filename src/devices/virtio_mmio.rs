//! virtio-MMIO transport (virtio 1.x §4.2.2). Holds the register state
//! every virtio device needs: features, status, the queue selector,
//! interrupt status. Device-class-specific behavior (config space,
//! queue draining) lives in the device file that owns this transport.

use super::virtio_queue::VirtioQueue;

const MAGIC_VALUE: u32 = 0x7472_6976;
const VERSION: u32 = 2;

/// Bit 32 of the device features word: VIRTIO_F_VERSION_1. Required by
/// every modern (1.0+) virtio device.
pub(super) const VIRTIO_F_VERSION_1: u64 = 1 << 32;
/// Our "INGO" vendor ID. Not a registered virtio vendor — purely
/// cosmetic, shows up in `/sys/bus/virtio/devices/.../vendor`.
pub(super) const VENDOR_ID: u32 = 0x494E_474F;
/// First byte of the device-specific config space, per virtio 1.x
/// §4.2.2. MMIO offsets 0x000–0x0fc are the common transport regs.
pub(super) const CONFIG_BASE: u64 = 0x100;
/// InterruptStatus bit 0: "used ring updated". The driver reads this
/// in its IRQ handler and ACKs by writing the same bit to 0x064.
pub(super) const INT_USED_RING: u32 = 1;

pub(crate) struct VirtioMmio {
    pub device_id: u32,
    pub vendor_id: u32,
    pub features: u64,
    pub status: u32,
    pub interrupt_status: u32,
    pub config_generation: u32,
    pub queues: Vec<VirtioQueue>,
    device_feat_sel: u32,
    driver_feat: u64,
    driver_feat_sel: u32,
    queue_sel: u32,
}

impl VirtioMmio {
    pub fn new(device_id: u32, vendor_id: u32, features: u64, num_queues: usize) -> Self {
        VirtioMmio {
            device_id, vendor_id, features,
            status: 0, interrupt_status: 0, config_generation: 0,
            queues: (0..num_queues).map(|_| VirtioQueue::new()).collect(),
            device_feat_sel: 0, driver_feat: 0, driver_feat_sel: 0, queue_sel: 0,
        }
    }

    pub fn read_reg(&self, offset: u64) -> u32 {
        match offset {
            0x000 => MAGIC_VALUE,
            0x004 => VERSION,
            0x008 => self.device_id,
            0x00c => self.vendor_id,
            0x010 => self.feature_slice(self.device_feat_sel, self.features),
            0x034 => self.cur_q().map(|q| q.num_max).unwrap_or(0),
            0x044 => self.cur_q().map(|q| q.ready).unwrap_or(0),
            0x060 => self.interrupt_status,
            0x070 => self.status,
            0x0fc => self.config_generation,
            _ => 0,
        }
    }

    /// Returns `Some(queue_index)` when the write was a notify kick the
    /// device layer must service; otherwise `None`.
    pub fn write_reg(&mut self, offset: u64, val: u32) -> Option<u32> {
        match offset {
            0x014 => self.device_feat_sel = val,
            0x020 => self.write_driver_feat(val),
            0x024 => self.driver_feat_sel = val,
            0x030 => self.queue_sel = val,
            0x038 => { if let Some(q) = self.cur_q_mut() { q.num = val; } }
            0x044 => { if let Some(q) = self.cur_q_mut() { q.ready = val; } }
            0x050 => return Some(val),
            0x064 => self.interrupt_status &= !val,
            0x070 => self.status = val,
            0x080 => { if let Some(q) = self.cur_q_mut() { q.set_desc_lo(val); } }
            0x084 => { if let Some(q) = self.cur_q_mut() { q.set_desc_hi(val); } }
            0x090 => { if let Some(q) = self.cur_q_mut() { q.set_avail_lo(val); } }
            0x094 => { if let Some(q) = self.cur_q_mut() { q.set_avail_hi(val); } }
            0x0a0 => { if let Some(q) = self.cur_q_mut() { q.set_used_lo(val); } }
            0x0a4 => { if let Some(q) = self.cur_q_mut() { q.set_used_hi(val); } }
            _ => {}
        }
        None
    }

    fn feature_slice(&self, sel: u32, features: u64) -> u32 {
        if sel == 0 { features as u32 } else if sel == 1 { (features >> 32) as u32 } else { 0 }
    }

    fn write_driver_feat(&mut self, val: u32) {
        let shift = if self.driver_feat_sel == 0 { 0 } else { 32 };
        let mask = 0xFFFF_FFFFu64 << shift;
        self.driver_feat = (self.driver_feat & !mask) | ((val as u64) << shift);
    }

    fn cur_q(&self) -> Option<&VirtioQueue> { self.queues.get(self.queue_sel as usize) }
    fn cur_q_mut(&mut self) -> Option<&mut VirtioQueue> { self.queues.get_mut(self.queue_sel as usize) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new() -> VirtioMmio { VirtioMmio::new(1, 0x1AF4, 1 << 32, 2) }

    #[test]
    fn magic_version_id_advertised() {
        let m = new();
        assert_eq!(m.read_reg(0x000), 0x7472_6976);
        assert_eq!(m.read_reg(0x004), 2);
        assert_eq!(m.read_reg(0x008), 1);
    }

    #[test]
    fn feature_select_routes_low_and_high_words() {
        let m = new();
        assert_eq!(m.read_reg(0x010), 0);
        let mut m = m;
        m.write_reg(0x014, 1);
        assert_eq!(m.read_reg(0x010), 1);
    }

    #[test]
    fn queue_sel_addresses_independent_queues() {
        let mut m = new();
        m.write_reg(0x030, 0);
        m.write_reg(0x080, 0xAAAA_AAAA);
        m.write_reg(0x030, 1);
        m.write_reg(0x080, 0xBBBB_BBBB);
        m.write_reg(0x030, 0);
        assert_eq!(m.queues[0].desc_addr & 0xFFFF_FFFF, 0xAAAA_AAAA);
        assert_eq!(m.queues[1].desc_addr & 0xFFFF_FFFF, 0xBBBB_BBBB);
    }

    #[test]
    fn queue_notify_returns_queue_index() {
        let mut m = new();
        assert_eq!(m.write_reg(0x050, 1), Some(1));
        assert_eq!(m.write_reg(0x070, 0xFF), None);
    }

    #[test]
    fn interrupt_ack_clears_status_bits() {
        let mut m = new();
        m.interrupt_status = 0b11;
        m.write_reg(0x064, 0b01);
        assert_eq!(m.read_reg(0x060), 0b10);
    }
}
