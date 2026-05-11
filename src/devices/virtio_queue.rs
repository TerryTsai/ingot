//! virtio split-virtqueue addresses (virtio 1.x §2.6). The descriptor
//! table, available ring, and used ring live in guest memory; this
//! struct just tracks where, how big, and whether the driver has
//! finished setting it up.

const QUEUE_NUM_MAX: u32 = 256;

pub(crate) struct VirtioQueue {
    pub num_max: u32,
    pub num: u32,
    pub ready: u32,
    pub desc_addr: u64,
    pub avail_addr: u64,
    pub used_addr: u64,
}

impl VirtioQueue {
    pub fn new() -> Self {
        Self {
            num_max: QUEUE_NUM_MAX,
            num: 0, ready: 0,
            desc_addr: 0, avail_addr: 0, used_addr: 0,
        }
    }

    pub fn set_desc_lo(&mut self, v: u32) { set_lo(&mut self.desc_addr, v); }
    pub fn set_desc_hi(&mut self, v: u32) { set_hi(&mut self.desc_addr, v); }
    pub fn set_avail_lo(&mut self, v: u32) { set_lo(&mut self.avail_addr, v); }
    pub fn set_avail_hi(&mut self, v: u32) { set_hi(&mut self.avail_addr, v); }
    pub fn set_used_lo(&mut self, v: u32) { set_lo(&mut self.used_addr, v); }
    pub fn set_used_hi(&mut self, v: u32) { set_hi(&mut self.used_addr, v); }
}

fn set_lo(a: &mut u64, v: u32) {
    *a = (*a & 0xFFFFFFFF_00000000) | (v as u64);
}

fn set_hi(a: &mut u64, v: u32) {
    *a = (*a & 0x00000000_FFFFFFFF) | ((v as u64) << 32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_unconfigured() {
        let q = VirtioQueue::new();
        assert_eq!(q.num, 0);
        assert_eq!(q.ready, 0);
        assert_eq!(q.desc_addr, 0);
    }

    #[test]
    fn lo_hi_pairs_assemble_64bit() {
        let mut q = VirtioQueue::new();
        q.set_desc_lo(0xDEAD_BEEF);
        q.set_desc_hi(0x1234_5678);
        assert_eq!(q.desc_addr, 0x1234_5678_DEAD_BEEF);
        q.set_desc_lo(0xCAFE_BABE);
        assert_eq!(q.desc_addr, 0x1234_5678_CAFE_BABE);
    }

    #[test]
    fn num_max_is_advertised() {
        assert_eq!(VirtioQueue::new().num_max, 256);
    }
}
