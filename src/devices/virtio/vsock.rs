//! virtio-vsock (virtio 1.x §5.10) over MMIO. The protocol itself runs
//! in the host kernel via `/dev/vhost-vsock`; this device is the bridge
//! that lets the guest's virtio_vsock driver discover the device via
//! standard MMIO probing and shuttles per-queue doorbells between
//! guest-MMIO writes and vhost kick/call eventfds. Used-ring updates
//! come back via the call eventfds, polled into `interrupt_status`.

use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

use super::core::mmio::{VirtioMmio, CONFIG_BASE, INT_USED_RING, VENDOR_ID, VIRTIO_F_VERSION_1};
use crate::devices::host::{VhostVsock, VringConfig};
use crate::devices::{GuestError, Interrupt, MmioDevice};
use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

const DEVICE_ID_VSOCK: u32 = 19;
const VIRTIO_VSOCK_GSI: u32 = 7;
const NUM_QUEUES: usize = 3;
// vhost-vsock backs queues 0 (rx) and 1 (tx). Queue 2 (event) is
// optional in the virtio-vsock spec; we expose 3 queues so the guest's
// driver-side init matches the spec, but the kernel's VSOCK_VQ_MAX is
// 2, so the event queue is local-only.
const VHOST_BACKED_QUEUES: usize = 2;
const STATUS_DRIVER_OK: u32 = 4;
const REG_QUEUE_READY: u64 = 0x044;
const REG_STATUS: u64 = 0x070;

pub(crate) struct Vsock {
    mmio: VirtioMmio,
    vhost: VhostVsock,
    guest_cid: u64,
    kicks: [OwnedFd; NUM_QUEUES],
    calls: [OwnedFd; NUM_QUEUES],
    configured: [bool; NUM_QUEUES],
    running: bool,
}

impl Vsock {
    pub fn new(guest_cid: u64, mem_size: u64, host_addr: u64) -> Result<Self, HostError> {
        let vhost = VhostVsock::open(guest_cid, 0, mem_size, host_addr)?;
        Ok(Vsock {
            mmio: VirtioMmio::new(DEVICE_ID_VSOCK, VENDOR_ID, VIRTIO_F_VERSION_1, NUM_QUEUES),
            vhost, guest_cid,
            kicks: [Self::eventfd()?, Self::eventfd()?, Self::eventfd()?],
            calls: [Self::eventfd()?, Self::eventfd()?, Self::eventfd()?],
            configured: [false; NUM_QUEUES], running: false,
        })
    }

    /// Tell vhost-vsock about each ready vring, then SET_RUNNING once
    /// the driver has finished its setup (DRIVER_OK bit + every backed
    /// queue ready). Called on every QueueReady / Status write so the
    /// order in which the guest sets things up doesn't matter.
    fn maybe_start(&mut self) -> Result<(), GuestError> {
        if self.running { return Ok(()); }
        for i in 0..VHOST_BACKED_QUEUES {
            if self.configured[i] { continue; }
            let q = &self.mmio.queues[i];
            if q.ready == 0 || q.num == 0 { continue; }
            self.vhost.configure_vring(VringConfig {
                idx: i as u32, num: q.num,
                desc: q.desc_addr, avail: q.avail_addr, used: q.used_addr,
                kick: self.kicks[i].as_raw_fd(), call: self.calls[i].as_raw_fd(),
            }).map_err(|_| GuestError::MalformedDescriptor)?;
            self.configured[i] = true;
        }
        if self.mmio.status & STATUS_DRIVER_OK == 0 { return Ok(()); }
        if !self.configured[..VHOST_BACKED_QUEUES].iter().all(|&c| c) { return Ok(()); }
        self.vhost.set_running(true).map_err(|_| GuestError::MalformedDescriptor)?;
        self.running = true;
        Ok(())
    }

    /// Pulse the kick eventfd for queue `idx`. vhost is poll'ing on
    /// the fd; the kernel processes the virtqueue when it sees the
    /// eventfd increment.
    fn kick(&self, idx: usize) {
        let one: u64 = 1;
        unsafe { libc::write(self.kicks[idx].as_raw_fd(), &one as *const _ as _, 8); }
    }

    /// Drain pending call eventfds (one per queue). Each non-zero read
    /// is a "used ring updated" notification from vhost; we coalesce
    /// them into one INT_USED_RING bit for the guest IRQ.
    fn drain_calls(&mut self) {
        for fd in &self.calls {
            let mut buf = [0u8; 8];
            let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr() as _, 8) };
            if n == 8 { self.mmio.interrupt_status |= INT_USED_RING; }
        }
    }

    fn eventfd() -> Result<OwnedFd, HostError> {
        let fd = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
        if fd < 0 { Err(HostError::VhostVsockOpenFailed(Self::errno())) }
        else { Ok(unsafe { OwnedFd::from_raw_fd(fd) }) }
    }

    fn errno() -> i32 { std::io::Error::last_os_error().raw_os_error().unwrap_or(0) }
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
            if (q as usize) < NUM_QUEUES { self.kick(q as usize); }
        }
        if offset == REG_QUEUE_READY || offset == REG_STATUS { self.maybe_start()?; }
        Ok(())
    }

    fn pending_interrupt(&mut self) -> Option<Interrupt> {
        if self.mmio.interrupt_status != 0 { Some(Interrupt { gsi: VIRTIO_VSOCK_GSI }) } else { None }
    }

    fn poll_io(&mut self, _mem: &mut Memory) -> Result<(), GuestError> {
        if self.running { self.drain_calls(); }
        Ok(())
    }
}

