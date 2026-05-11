//! virtio-vsock (virtio 1.x §5.10) over MMIO. The protocol itself runs
//! in the host kernel via `/dev/vhost-vsock`; this device is the bridge
//! that lets the guest's virtio_vsock driver discover the device via
//! standard MMIO probing and shuttles per-queue doorbells between
//! guest-MMIO writes and vhost kick/call eventfds. Used-ring updates
//! come back via the call eventfds, polled into `interrupt_status`.

use std::os::fd::{FromRawFd, OwnedFd};

use super::vhost_vsock::VhostVsock;
use super::virtio_mmio::VirtioMmio;
use super::vsock_io::{drain_calls, kick, maybe_start};
use super::{GuestError, Interrupt, MmioDevice};
use crate::error::HostError;
use crate::memory::Memory;

const DEVICE_ID_VSOCK: u32 = 19;
const VENDOR_ID: u32 = 0x494E_474F;
const VIRTIO_F_VERSION_1: u64 = 1 << 32;
const NUM_QUEUES: usize = 3;
const CONFIG_BASE: u64 = 0x100;
const VIRTIO_VSOCK_GSI: u32 = 7;

pub(crate) struct Vsock {
    pub(super) mmio: VirtioMmio,
    pub(super) vhost: VhostVsock,
    pub(super) guest_cid: u64,
    pub(super) kicks: [OwnedFd; NUM_QUEUES],
    pub(super) calls: [OwnedFd; NUM_QUEUES],
    pub(super) configured: [bool; NUM_QUEUES],
    pub(super) running: bool,
}

impl Vsock {
    pub fn new(guest_cid: u64, mem_size: u64, host_addr: u64) -> Result<Self, HostError> {
        let vhost = VhostVsock::open(guest_cid, 0, mem_size, host_addr)?;
        Ok(Vsock {
            mmio: VirtioMmio::new(DEVICE_ID_VSOCK, VENDOR_ID, VIRTIO_F_VERSION_1, NUM_QUEUES),
            vhost, guest_cid,
            kicks: [event()?, event()?, event()?],
            calls: [event()?, event()?, event()?],
            configured: [false; NUM_QUEUES], running: false,
        })
    }
}

impl MmioDevice for Vsock {
    fn read(&mut self, offset: u64, data: &mut [u8]) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE {
            let cid = self.guest_cid.to_le_bytes();
            for (i, b) in data.iter_mut().enumerate() {
                let abs = (offset - CONFIG_BASE) as usize + i;
                *b = if abs < cid.len() { cid[abs] } else { 0 };
            }
            return Ok(());
        }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        data.copy_from_slice(&self.mmio.read_reg(offset).to_le_bytes());
        Ok(())
    }

    fn write(&mut self, offset: u64, data: &[u8], _mem: &mut Memory) -> Result<(), GuestError> {
        if offset >= CONFIG_BASE { return Ok(()); }
        if data.len() != 4 { return Err(GuestError::MmioOutOfRange { addr: offset }); }
        let v = u32::from_le_bytes(data.try_into().unwrap());
        if let Some(q) = self.mmio.write_reg(offset, v) {
            if (q as usize) < NUM_QUEUES { kick(self, q as usize); }
        }
        if offset == 0x044 || offset == 0x070 { maybe_start(self)?; }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_VSOCK_GSI }) } else { None }
    }

    fn poll_io(&mut self, _mem: &mut Memory) -> Result<(), GuestError> {
        if self.running { drain_calls(self); }
        Ok(())
    }
}

fn event() -> Result<OwnedFd, HostError> {
    let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if fd < 0 { Err(HostError::VhostVsockOpenFailed(errno())) } else { Ok(unsafe { OwnedFd::from_raw_fd(fd) }) }
}

fn errno() -> i32 { std::io::Error::last_os_error().raw_os_error().unwrap_or(0) }
