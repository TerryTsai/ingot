//! virtio-net host-side data path. `drain_tx` walks the TX virtqueue's
//! available chains, gathers each L2 frame (skipping the leading
//! 12-byte vnet_hdr), and writes it to the TAP fd. `drain_rx` does the
//! inverse on non-blocking reads from the TAP fd: prefix a zeroed
//! vnet_hdr and scatter into the next free RX descriptor chain.

use super::net::Net;
use super::virtio_chain::{read_chain, write_chain};
use super::GuestError;
use crate::memory::Memory;

const VNET_HDR_LEN: usize = 12;
const FRAME_MAX: usize = 2048;
const QUEUE_RX: usize = 0;
const QUEUE_TX: usize = 1;
const INT_USED_RING: u32 = 1;

pub(super) fn drain_tx(n: &mut Net, mem: &mut Memory) -> Result<(), GuestError> {
    let q = &mut n.mmio.queues[QUEUE_TX];
    let mut frame = Vec::with_capacity(FRAME_MAX);
    let mut serviced = 0u32;
    while let Some(head) = q.pop_avail(mem)? {
        frame.clear();
        read_chain(q, mem, head, &mut frame)?;
        if let Some(tap) = &n.tap {
            if frame.len() > VNET_HDR_LEN { let _ = tap.write(&frame[VNET_HDR_LEN..]); }
        }
        let total = frame.len() as u32;
        q.push_used(mem, head, total)?;
        n.tx_packets += 1; n.tx_bytes += total as u64; serviced += 1;
    }
    if serviced > 0 { n.mmio.interrupt_status |= INT_USED_RING; }
    Ok(())
}

pub(super) fn drain_rx(n: &mut Net, mem: &mut Memory) -> Result<(), GuestError> {
    let tap = match &n.tap { Some(t) => t, None => return Ok(()) };
    let mut buf = [0u8; FRAME_MAX];
    let mut serviced = 0u32;
    loop {
        let n_read = tap.read(&mut buf).map_err(|_| GuestError::MalformedDescriptor)?;
        if n_read == 0 { break; }
        let q = &mut n.mmio.queues[QUEUE_RX];
        let head = match q.pop_avail(mem)? { Some(h) => h, None => break };
        let mut framed = Vec::with_capacity(VNET_HDR_LEN + n_read);
        framed.extend_from_slice(&[0u8; VNET_HDR_LEN]);
        framed.extend_from_slice(&buf[..n_read]);
        let w = write_chain(q, mem, head, &framed)?;
        q.push_used(mem, head, w)?;
        serviced += 1;
    }
    if serviced > 0 { n.mmio.interrupt_status |= INT_USED_RING; }
    Ok(())
}
