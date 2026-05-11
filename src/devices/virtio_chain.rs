//! Buffer transfer across a descriptor chain. The chain rooted at
//! `head` is a linked list (`NEXT` flag + `next` index) of guest-memory
//! buffers; `read_chain` concatenates them into a host buffer (TX path)
//! and `write_chain` scatters a host buffer back across writable
//! descriptors (RX path). Iteration is capped at the queue size to
//! defeat a guest that builds a cyclic chain.

use super::virtio_queue::{VirtioQueue, DESC_F_NEXT, DESC_F_WRITE};
use super::GuestError;
use crate::memory::Memory;

pub(crate) fn read_chain(
    q: &VirtioQueue,
    mem: &Memory,
    head: u16,
    out: &mut Vec<u8>,
) -> Result<(), GuestError> {
    let mut idx = head;
    let cap = q.num as usize;
    for _ in 0..cap {
        let d = q.read_desc(mem, idx)?;
        let start = out.len();
        out.resize(start + d.len as usize, 0);
        mem.read(d.addr, &mut out[start..])?;
        if d.flags & DESC_F_NEXT == 0 { return Ok(()); }
        idx = d.next;
    }
    Err(GuestError::DescriptorChainCycle)
}

/// Writes `data` into the writable descriptors of the chain. Returns
/// the number of bytes actually placed (may be less than `data.len()`
/// if the chain is shorter than the payload — the caller decides what
/// that means for its protocol).
pub(crate) fn write_chain(
    q: &VirtioQueue,
    mem: &mut Memory,
    head: u16,
    data: &[u8],
) -> Result<u32, GuestError> {
    let mut idx = head;
    let mut written = 0usize;
    let cap = q.num as usize;
    for _ in 0..cap {
        if written >= data.len() { return Ok(written as u32); }
        let d = q.read_desc(mem, idx)?;
        if d.flags & DESC_F_WRITE == 0 { return Err(GuestError::MalformedDescriptor); }
        let take = (d.len as usize).min(data.len() - written);
        mem.write(d.addr, &data[written..written + take])?;
        written += take;
        if d.flags & DESC_F_NEXT == 0 { return Ok(written as u32); }
        idx = d.next;
    }
    Err(GuestError::DescriptorChainCycle)
}

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
    fn read_chain_concatenates_two_buffers() {
        let q = configured(4);
        let mut m = mem();
        plant_desc(&mut m, &q, 0, 0x4000, 3, DESC_F_NEXT, 1);
        plant_desc(&mut m, &q, 1, 0x5000, 2, 0, 0);
        m.write(0x4000, b"abc").unwrap();
        m.write(0x5000, b"de").unwrap();
        let mut out = Vec::new();
        read_chain(&q, &m, 0, &mut out).unwrap();
        assert_eq!(out, b"abcde".to_vec());
    }

    #[test]
    fn write_chain_scatters_into_writable_descriptors() {
        let q = configured(4);
        let mut m = mem();
        plant_desc(&mut m, &q, 0, 0x4000, 3, DESC_F_NEXT | DESC_F_WRITE, 1);
        plant_desc(&mut m, &q, 1, 0x5000, 3, DESC_F_WRITE, 0);
        let n = write_chain(&q, &mut m, 0, b"abcde").unwrap();
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
        assert!(write_chain(&q, &mut m, 0, b"xyz").is_err());
    }
}
