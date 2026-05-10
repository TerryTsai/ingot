//! Guest physical memory mapping.
//!
//! A single mmap-backed region [0, size). MMIO is outside this range
//! and is not backed by GuestMem.

use std::io;

use crate::devices::GuestError;
use crate::error::HostError;

pub struct GuestMem {
    ptr: *mut u8,
    size: u64,
}

impl GuestMem {
    pub fn new(size_bytes: u64) -> Result<Self, HostError> {
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size_bytes as usize,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(HostError::MmapFailed(errno()));
        }
        Ok(GuestMem { ptr: p as *mut u8, size: size_bytes })
    }

    pub fn write(&mut self, guest_phys: u64, data: &[u8]) -> Result<(), GuestError> {
        self.bounds_check(guest_phys, data.len())?;
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr.add(guest_phys as usize), data.len());
        }
        Ok(())
    }

    pub fn read(&self, guest_phys: u64, dst: &mut [u8]) -> Result<(), GuestError> {
        self.bounds_check(guest_phys, dst.len())?;
        unsafe {
            std::ptr::copy_nonoverlapping(self.ptr.add(guest_phys as usize), dst.as_mut_ptr(), dst.len());
        }
        Ok(())
    }

    pub fn host_ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    fn bounds_check(&self, guest_phys: u64, len: usize) -> Result<(), GuestError> {
        let end = guest_phys.checked_add(len as u64);
        match end {
            Some(e) if e <= self.size => Ok(()),
            _ => Err(GuestError::InvalidGuestPhys { addr: guest_phys, len }),
        }
    }
}

impl Drop for GuestMem {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as _, self.size as usize);
        }
    }
}

fn errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[cfg(test)]
mod tests;
