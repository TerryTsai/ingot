//! virtio-block (virtio 1.x §5.2) over MMIO. One virtqueue, file-backed.
//! Each notify drains the queue; per-request layout (header / data /
//! status) and read/write/flush dispatch live in `block_io.rs`. Config
//! space exposes capacity in 512-byte sectors so the kernel sizes
//! `/dev/vda` correctly at probe.

use std::fs::{File, OpenOptions};
use std::path::Path;

use super::block_io::process_requests;
use super::virtio_mmio::VirtioMmio;
use super::{GuestError, Interrupt, MmioDevice};
use crate::error::HostError;
use crate::memory::Memory;

const DEVICE_ID_BLOCK: u32 = 2;
const VENDOR_ID: u32 = 0x494E_474F;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const VIRTIO_BLK_F_RO: u64 = 1 << 5;
const NUM_QUEUES: usize = 1;
const CONFIG_BASE: u64 = 0x100;
const SECTOR: u64 = 512;
const VIRTIO_BLK_GSI: u32 = 6;

pub(crate) struct Block {
    pub(super) mmio: VirtioMmio,
    pub(super) file: File,
    pub(super) readonly: bool,
    pub(super) capacity_sectors: u64,
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
        if self.mmio.write_reg(offset, v).is_some() { process_requests(self, mem)?; }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_BLK_GSI }) } else { None }
    }
}
