//! virtio split-virtqueue (virtio 1.x §2.6). Holds the queue's addresses
//! and per-queue state; provides hostile-guest-safe accessors that read
//! the available ring, follow descriptor chains, and write the used ring.
//! `read_chain` and `write_chain` walk a `NEXT`-linked list of
//! descriptors, capping iteration at the queue size so a cyclic chain
//! returns `DescriptorChainCycle` instead of looping forever.

use super::GuestError;
use crate::memory::Memory;

const QUEUE_NUM_MAX: u32 = 256;
const DESC_SIZE: u64 = 16;
const USED_ELEM_SIZE: u64 = 8;

pub(crate) const DESC_F_NEXT: u16 = 1;
pub(crate) const DESC_F_WRITE: u16 = 2;

pub(crate) struct VirtioQueue {
    pub num_max: u32,
    pub num: u32,
    pub ready: u32,
    pub desc_addr: u64,
    pub avail_addr: u64,
    pub used_addr: u64,
    last_avail_idx: u16,
}

pub(crate) struct Desc {
    pub addr: u64,
    pub len: u32,
    pub flags: u16,
    pub next: u16,
}

impl VirtioQueue {
    pub fn new() -> Self {
        VirtioQueue {
            num_max: QUEUE_NUM_MAX, num: 0, ready: 0,
            desc_addr: 0, avail_addr: 0, used_addr: 0,
            last_avail_idx: 0,
        }
    }

    pub fn set_desc_lo(&mut self, v: u32) { set_lo(&mut self.desc_addr, v); }
    pub fn set_desc_hi(&mut self, v: u32) { set_hi(&mut self.desc_addr, v); }
    pub fn set_avail_lo(&mut self, v: u32) { set_lo(&mut self.avail_addr, v); }
    pub fn set_avail_hi(&mut self, v: u32) { set_hi(&mut self.avail_addr, v); }
    pub fn set_used_lo(&mut self, v: u32) { set_lo(&mut self.used_addr, v); }
    pub fn set_used_hi(&mut self, v: u32) { set_hi(&mut self.used_addr, v); }

    /// Pop the next descriptor-chain head from the available ring, if
    /// one has appeared since the last call. Returns `None` if empty,
    /// errors on malformed indices.
    pub fn pop_avail(&mut self, mem: &Memory) -> Result<Option<u16>, GuestError> {
        if self.ready == 0 || self.num == 0 { return Ok(None); }
        let avail_idx = read_u16(mem, self.avail_addr + 2)?;
        if avail_idx == self.last_avail_idx { return Ok(None); }
        let ring_off = self.avail_addr + 4 + ((self.last_avail_idx as u64) % self.num as u64) * 2;
        let head = read_u16(mem, ring_off)?;
        if (head as u32) >= self.num { return Err(GuestError::MalformedDescriptor); }
        self.last_avail_idx = self.last_avail_idx.wrapping_add(1);
        Ok(Some(head))
    }

    pub fn read_desc(&self, mem: &Memory, idx: u16) -> Result<Desc, GuestError> {
        if (idx as u32) >= self.num { return Err(GuestError::MalformedDescriptor); }
        let off = self.desc_addr + idx as u64 * DESC_SIZE;
        Ok(Desc {
            addr: read_u64(mem, off)?,
            len: read_u32(mem, off + 8)?,
            flags: read_u16(mem, off + 12)?,
            next: read_u16(mem, off + 14)?,
        })
    }

    pub fn push_used(&self, mem: &mut Memory, id: u16, len: u32) -> Result<(), GuestError> {
        let used_idx = read_u16(mem, self.used_addr + 2)?;
        let ring_off = self.used_addr + 4 + ((used_idx as u64) % self.num as u64) * USED_ELEM_SIZE;
        write_u32(mem, ring_off, id as u32)?;
        write_u32(mem, ring_off + 4, len)?;
        write_u16(mem, self.used_addr + 2, used_idx.wrapping_add(1))?;
        Ok(())
    }

    /// Concatenate all buffers in the chain rooted at `head` into `out`.
    /// Used by device-class TX paths (driver-filled buffers to ship out).
    pub fn read_chain(&self, mem: &Memory, head: u16, out: &mut Vec<u8>) -> Result<(), GuestError> {
        let mut idx = head;
        for _ in 0..self.num as usize {
            let d = self.read_desc(mem, idx)?;
            let start = out.len();
            out.resize(start + d.len as usize, 0);
            mem.read(d.addr, &mut out[start..])?;
            if d.flags & DESC_F_NEXT == 0 { return Ok(()); }
            idx = d.next;
        }
        Err(GuestError::DescriptorChainCycle)
    }

    /// Scatter `data` across the writable descriptors of the chain
    /// rooted at `head`. Used by device-class RX paths. Returns the
    /// byte count placed (may be less than `data.len()` if the chain
    /// is shorter than the payload).
    pub fn write_chain(&self, mem: &mut Memory, head: u16, data: &[u8]) -> Result<u32, GuestError> {
        let mut idx = head;
        let mut written = 0usize;
        for _ in 0..self.num as usize {
            if written >= data.len() { return Ok(written as u32); }
            let d = self.read_desc(mem, idx)?;
            if d.flags & DESC_F_WRITE == 0 { return Err(GuestError::MalformedDescriptor); }
            let take = (d.len as usize).min(data.len() - written);
            mem.write(d.addr, &data[written..written + take])?;
            written += take;
            if d.flags & DESC_F_NEXT == 0 { return Ok(written as u32); }
            idx = d.next;
        }
        Err(GuestError::DescriptorChainCycle)
    }
}

fn set_lo(a: &mut u64, v: u32) { *a = (*a & 0xFFFFFFFF_00000000) | (v as u64); }
fn set_hi(a: &mut u64, v: u32) { *a = (*a & 0x00000000_FFFFFFFF) | ((v as u64) << 32); }

fn read_u16(m: &Memory, a: u64) -> Result<u16, GuestError> {
    let mut b = [0u8; 2]; m.read(a, &mut b)?; Ok(u16::from_le_bytes(b))
}
fn read_u32(m: &Memory, a: u64) -> Result<u32, GuestError> {
    let mut b = [0u8; 4]; m.read(a, &mut b)?; Ok(u32::from_le_bytes(b))
}
fn read_u64(m: &Memory, a: u64) -> Result<u64, GuestError> {
    let mut b = [0u8; 8]; m.read(a, &mut b)?; Ok(u64::from_le_bytes(b))
}
fn write_u16(m: &mut Memory, a: u64, v: u16) -> Result<(), GuestError> { m.write(a, &v.to_le_bytes()) }
fn write_u32(m: &mut Memory, a: u64, v: u32) -> Result<(), GuestError> { m.write(a, &v.to_le_bytes()) }

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Memory { Memory::new(0x10_0000).unwrap() }

    fn configured(num: u32) -> VirtioQueue {
        let mut q = VirtioQueue::new();
        q.num = num; q.ready = 1;
        q.desc_addr = 0x1000;
        q.avail_addr = 0x2000;
        q.used_addr = 0x3000;
        q
    }

    fn plant_desc(m: &mut Memory, q: &VirtioQueue, idx: u16, addr: u64, len: u32, flags: u16, next: u16) {
        let off = q.desc_addr + idx as u64 * 16;
        m.write(off, &addr.to_le_bytes()).unwrap();
        m.write(off + 8, &len.to_le_bytes()).unwrap();
        m.write(off + 12, &flags.to_le_bytes()).unwrap();
        m.write(off + 14, &next.to_le_bytes()).unwrap();
    }

    #[test]
    fn pop_avail_returns_none_when_idx_unchanged() {
        let mut q = configured(8);
        let m = mem();
        assert_eq!(q.pop_avail(&m).unwrap(), None);
    }

    #[test]
    fn pop_avail_returns_head_when_driver_advances_idx() {
        let mut q = configured(8);
        let mut m = mem();
        m.write(q.avail_addr + 4, &3u16.to_le_bytes()).unwrap();
        m.write(q.avail_addr + 2, &1u16.to_le_bytes()).unwrap();
        assert_eq!(q.pop_avail(&m).unwrap(), Some(3));
        assert_eq!(q.pop_avail(&m).unwrap(), None);
    }

    #[test]
    fn read_desc_rejects_oob_index() {
        let q = configured(4);
        let m = mem();
        assert!(q.read_desc(&m, 5).is_err());
    }

    #[test]
    fn push_used_writes_id_len_and_advances_idx() {
        let q = configured(4);
        let mut m = mem();
        q.push_used(&mut m, 2, 1500).unwrap();
        let mut buf = [0u8; 4];
        m.read(q.used_addr + 4, &mut buf).unwrap();
        assert_eq!(u32::from_le_bytes(buf), 2);
        m.read(q.used_addr + 8, &mut buf).unwrap();
        assert_eq!(u32::from_le_bytes(buf), 1500);
        m.read(q.used_addr + 2, &mut buf[..2]).unwrap();
        assert_eq!(u16::from_le_bytes([buf[0], buf[1]]), 1);
    }

    #[test]
    fn read_chain_concatenates_two_buffers() {
        let q = configured(4);
        let mut m = mem();
        plant_desc(&mut m, &q, 0, 0x4000, 3, DESC_F_NEXT, 1);
        plant_desc(&mut m, &q, 1, 0x5000, 2, 0, 0);
        m.write(0x4000, b"abc").unwrap();
        m.write(0x5000, b"de").unwrap();
        let mut out = Vec::new();
        q.read_chain(&m, 0, &mut out).unwrap();
        assert_eq!(out, b"abcde".to_vec());
    }

    #[test]
    fn write_chain_scatters_into_writable_descriptors() {
        let q = configured(4);
        let mut m = mem();
        plant_desc(&mut m, &q, 0, 0x4000, 3, DESC_F_NEXT | DESC_F_WRITE, 1);
        plant_desc(&mut m, &q, 1, 0x5000, 3, DESC_F_WRITE, 0);
        let n = q.write_chain(&mut m, 0, b"abcde").unwrap();
        assert_eq!(n, 5);
        let mut buf = [0u8; 3];
        m.read(0x4000, &mut buf).unwrap();
        assert_eq!(&buf, b"abc");
        m.read(0x5000, &mut buf).unwrap();
        assert_eq!(&buf[..2], b"de");
    }

    #[test]
    fn write_chain_rejects_read_only_descriptor() {
        let q = configured(4);
        let mut m = mem();
        plant_desc(&mut m, &q, 0, 0x4000, 3, 0, 0);
        assert!(q.write_chain(&mut m, 0, b"xyz").is_err());
    }
}
