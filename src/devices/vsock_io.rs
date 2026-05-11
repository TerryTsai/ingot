//! virtio-vsock host-side glue. The guest's MMIO writes to `QueueReady`
//! and `Status::DRIVER_OK` come through here as `maybe_start` / queue
//! config; `kick` and `drain_calls` are the eventfd doorbells between
//! the VMM and vhost-vsock.

use std::os::fd::AsRawFd;

use super::vsock::Vsock;
use super::GuestError;

const STATUS_DRIVER_OK: u32 = 4;
const INT_USED_RING: u32 = 1;
// vhost-vsock backs queues 0 (rx) and 1 (tx). Queue 2 (event) is
// optional in the virtio-vsock spec; we expose 3 queues so the guest's
// driver-side init matches the spec, but we don't hand queue 2 to
// vhost — the kernel's VSOCK_VQ_MAX is 2.
const VHOST_BACKED_QUEUES: usize = 2;

pub(super) fn maybe_start(v: &mut Vsock) -> Result<(), GuestError> {
    if v.running { return Ok(()); }
    for i in 0..VHOST_BACKED_QUEUES {
        if v.configured[i] { continue; }
        let q = &v.mmio.queues[i];
        if q.ready == 0 || q.num == 0 { continue; }
        v.vhost.configure_vring(i as u32, q.num, q.desc_addr, q.avail_addr, q.used_addr,
                                v.kicks[i].as_raw_fd(), v.calls[i].as_raw_fd())
            .map_err(|_| GuestError::MalformedDescriptor)?;
        v.configured[i] = true;
    }
    if v.mmio.status & STATUS_DRIVER_OK == 0 { return Ok(()); }
    if !v.configured[..VHOST_BACKED_QUEUES].iter().all(|&c| c) { return Ok(()); }
    v.vhost.set_running(true).map_err(|_| GuestError::MalformedDescriptor)?;
    v.running = true;
    Ok(())
}

pub(super) fn kick(v: &Vsock, idx: usize) {
    let one: u64 = 1;
    unsafe { libc::write(v.kicks[idx].as_raw_fd(), &one as *const _ as _, 8); }
}

pub(super) fn drain_calls(v: &mut Vsock) {
    for fd in &v.calls {
        let mut buf = [0u8; 8];
        let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr() as _, 8) };
        if n == 8 { v.mmio.interrupt_status |= INT_USED_RING; }
    }
}
