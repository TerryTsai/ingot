//! virtio-block (virtio 1.x §5.2) over MMIO. One virtqueue, file-backed.
//! Each request is a descriptor chain laid out as: read-only 16-byte
//! header, zero or more data buffers (read-only for OUT, write-only for
//! IN), one write-only status byte. We classify descriptors by their
//! `WRITE` flag and chain position rather than by hardcoded index —
//! drivers occasionally split buffers across multiple descriptors.
//! Config space exposes capacity in 512-byte sectors so the kernel
//! sizes `/dev/vda` correctly at probe.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::virtio_mmio::{VirtioMmio, CONFIG_BASE, INT_USED_RING, VENDOR_ID, VIRTIO_F_VERSION_1};
use super::virtio_queue::{Desc, DESC_F_NEXT, DESC_F_WRITE};
use super::{GuestError, Interrupt, MmioDevice};
use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

const DEVICE_ID_BLOCK: u32 = 2;
const VIRTIO_BLK_F_RO: u64 = 1 << 5;
const VIRTIO_BLK_GSI: u32 = 6;
const NUM_QUEUES: usize = 1;
const QUEUE_IDX: usize = 0;
const SECTOR: u64 = 512;

const VIRTIO_BLK_T_IN: u32 = 0;
const VIRTIO_BLK_T_OUT: u32 = 1;
const VIRTIO_BLK_T_FLUSH: u32 = 4;
const STATUS_OK: u8 = 0;
const STATUS_IOERR: u8 = 1;
const STATUS_UNSUPP: u8 = 2;
const HEADER_LEN: u32 = 16;

pub(crate) struct Block {
    mmio: VirtioMmio,
    file: File,
    readonly: bool,
    capacity_sectors: u64,
}

impl Block {
    pub fn new(path: &Path, readonly: bool) -> Result<Self, HostError> {
        let file = OpenOptions::new().read(true).write(!readonly).open(path)
            .map_err(|_| HostError::RootfsNotReadable(path.to_path_buf()))?;
        let size = file.metadata().map_err(|_| HostError::RootfsNotReadable(path.to_path_buf()))?.len();
        let capacity_sectors = size / SECTOR;
        let mut features = VIRTIO_F_VERSION_1;
        if readonly { features |= VIRTIO_BLK_F_RO; }
        Ok(Block {
            mmio: VirtioMmio::new(DEVICE_ID_BLOCK, VENDOR_ID, features, NUM_QUEUES),
            file, readonly, capacity_sectors,
        })
    }

    fn read_config(&self, off: u64, data: &mut [u8]) {
        let cap = self.capacity_sectors.to_le_bytes();
        for (i, b) in data.iter_mut().enumerate() {
            let abs = off as usize + i;
            *b = if abs < cap.len() { cap[abs] } else { 0 };
        }
    }

    fn process_requests(&mut self, mem: &mut Memory) -> Result<(), GuestError> {
        let mut serviced = 0u32;
        while let Some(head) = self.mmio.queues[QUEUE_IDX].pop_avail(mem)? {
            let (header_desc, data_descs, status_desc) = self.split_chain(mem, head)?;
            let (req_type, sector) = Self::read_header(mem, &header_desc)?;
            let (status, written) = self.service(mem, req_type, sector, &data_descs);
            mem.write(status_desc.addr, &[status])?;
            self.mmio.queues[QUEUE_IDX].push_used(mem, head, written + 1)?;
            serviced += 1;
        }
        if serviced > 0 { self.mmio.interrupt_status |= INT_USED_RING; }
        Ok(())
    }

    fn split_chain(&self, mem: &Memory, head: u16) -> Result<(Desc, Vec<Desc>, Desc), GuestError> {
        let q = &self.mmio.queues[QUEUE_IDX];
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

    fn service(&mut self, mem: &mut Memory, req_type: u32, sector: u64, data: &[Desc]) -> (u8, u32) {
        match req_type {
            VIRTIO_BLK_T_IN => self.do_io(mem, sector, data, true).unwrap_or((STATUS_IOERR, 0)),
            VIRTIO_BLK_T_OUT if self.readonly => (STATUS_IOERR, 0),
            VIRTIO_BLK_T_OUT => self.do_io(mem, sector, data, false).unwrap_or((STATUS_IOERR, 0)),
            VIRTIO_BLK_T_FLUSH => { let _ = self.file.sync_data(); (STATUS_OK, 0) }
            _ => (STATUS_UNSUPP, 0),
        }
    }

    fn do_io(&mut self, mem: &mut Memory, sector: u64, data: &[Desc], read: bool) -> std::io::Result<(u8, u32)> {
        let mut off = sector * SECTOR;
        let mut total = 0u32;
        for d in data {
            let writable = d.flags & DESC_F_WRITE != 0;
            if writable != read { return Ok((STATUS_IOERR, 0)); }
            let mut buf = vec![0u8; d.len as usize];
            self.file.seek(SeekFrom::Start(off))?;
            if read {
                self.file.read_exact(&mut buf)?;
                mem.write(d.addr, &buf).map_err(|_| std::io::ErrorKind::Other)?;
            } else {
                mem.read(d.addr, &mut buf).map_err(|_| std::io::ErrorKind::Other)?;
                self.file.write_all(&buf)?;
            }
            off += d.len as u64;
            total += d.len;
        }
        Ok((STATUS_OK, total))
    }

    fn read_header(mem: &Memory, h: &Desc) -> Result<(u32, u64), GuestError> {
        let mut buf = [0u8; HEADER_LEN as usize];
        mem.read(h.addr, &mut buf)?;
        let req_type = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let sector = u64::from_le_bytes([buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15]]);
        Ok((req_type, sector))
    }
}

impl MmioDevice for Block {
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
        if self.mmio.write_reg(offset, v).is_some() { self.process_requests(mem)?; }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_BLK_GSI }) } else { None }
    }
}
