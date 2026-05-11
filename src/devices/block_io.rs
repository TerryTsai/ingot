//! virtio-block request processing (virtio 1.x §5.2). Each request is a
//! descriptor chain laid out as: read-only 16-byte header, zero or more
//! data buffers (read-only for OUT, write-only for IN), one write-only
//! status byte. We classify descriptors by their `WRITE` flag and chain
//! position rather than by hardcoded index — drivers occasionally split
//! buffers across multiple descriptors.

use std::io::{Read, Seek, SeekFrom, Write};

use super::block::Block;
use super::virtio_queue::{Desc, DESC_F_NEXT, DESC_F_WRITE};
use super::GuestError;
use crate::memory::Memory;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;
const STATUS_OK: u8 = 0;
const STATUS_IOERR: u8 = 1;
const STATUS_UNSUPP: u8 = 2;
const HEADER_LEN: u32 = 16;
const SECTOR: u64 = 512;
const INT_USED_RING: u32 = 1;
const QUEUE_IDX: usize = 0;

pub(super) fn process_requests(b: &mut Block, mem: &mut Memory) -> Result<(), GuestError> {
    let mut serviced = 0u32;
    while let Some(head) = b.mmio.queues[QUEUE_IDX].pop_avail(mem)? {
        let (header_desc, data_descs, status_desc) = split_chain(b, mem, head)?;
        let (req_type, sector) = read_header(mem, &header_desc)?;
        let (status, written) = service(b, mem, req_type, sector, &data_descs);
        mem.write(status_desc.addr, &[status])?;
        b.mmio.queues[QUEUE_IDX].push_used(mem, head, written + 1)?;
        serviced += 1;
    }
    if serviced > 0 { b.mmio.interrupt_status |= INT_USED_RING; }
    Ok(())
}

fn split_chain(b: &Block, mem: &Memory, head: u16) -> Result<(Desc, Vec<Desc>, Desc), GuestError> {
    let q = &b.mmio.queues[QUEUE_IDX];
    let cap = q.num as usize;
    let mut descs = Vec::with_capacity(cap);
    let mut idx = head;
    for _ in 0..cap {
        let d = q.read_desc(mem, idx)?;
        let has_next = d.flags & DESC_F_NEXT != 0;
        let next = d.next;
        descs.push(d);
        if !has_next { break; }
        idx = next;
    }
    if descs.len() < 2 { return Err(GuestError::MalformedDescriptor); }
    let status = descs.pop().unwrap();
    let header = descs.remove(0);
    if header.flags & DESC_F_WRITE != 0 || header.len != HEADER_LEN { return Err(GuestError::MalformedDescriptor); }
    if status.flags & DESC_F_WRITE == 0 || status.len < 1 { return Err(GuestError::MalformedDescriptor); }
    Ok((header, descs, status))
}

fn read_header(mem: &Memory, h: &Desc) -> Result<(u32, u64), GuestError> {
    let mut buf = [0u8; HEADER_LEN as usize];
    mem.read(h.addr, &mut buf)?;
    let req_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let sector = u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]);
    Ok((req_type, sector))
}

fn service(b: &mut Block, mem: &mut Memory, req_type: u32, sector: u64, data: &[Desc]) -> (u8, u32) {
    match req_type {
        VIRTIO_BLK_T_IN => do_io(b, mem, sector, data, true).unwrap_or((STATUS_IOERR, 0)),
        VIRTIO_BLK_T_OUT if b.readonly => (STATUS_IOERR, 0),
        VIRTIO_BLK_T_OUT => do_io(b, mem, sector, data, false).unwrap_or((STATUS_IOERR, 0)),
        VIRTIO_BLK_T_FLUSH => { let _ = b.file.sync_data(); (STATUS_OK, 0) }
        _ => (STATUS_UNSUPP, 0),
    }
}

fn do_io(b: &mut Block, mem: &mut Memory, sector: u64, data: &[Desc], read: bool) -> std::io::Result<(u8, u32)> {
    let mut off = sector * SECTOR;
    let mut total = 0u32;
    for d in data {
        let writable = d.flags & DESC_F_WRITE != 0;
        if writable != read { return Ok((STATUS_IOERR, 0)); }
        let mut buf = vec![0u8; d.len as usize];
        b.file.seek(SeekFrom::Start(off))?;
        if read {
            b.file.read_exact(&mut buf)?;
            mem.write(d.addr, &buf).map_err(|_| std::io::ErrorKind::Other)?;
        } else {
            mem.read(d.addr, &mut buf).map_err(|_| std::io::ErrorKind::Other)?;
            b.file.write_all(&buf)?;
        }
        off += d.len as u64;
        total += d.len;
    }
    Ok((STATUS_OK, total))
}
