//! Guest physical memory: a single mmap-backed region [0, size). MMIO
//! lives outside this range and is not backed by `Memory`.

use std::io;

use crate::devices::GuestError;
use crate::lifecycle::error::HostError;

pub(crate) struct Memory {
    ptr: *mut u8,
    size: u64,
}

impl Memory {
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
        Ok(Memory { ptr: p as *mut u8, size: size_bytes })
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
        match guest_phys.checked_add(len as u64) {
            Some(end) if end <= self.size => Ok(()),
            _ => Err(GuestError::InvalidGuestPhys { addr: guest_phys, len }),
        }
    }
}

impl Drop for Memory {
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
mod tests {
    use super::*;

    const ONE_MB: u64 = 1024 * 1024;

    #[test]
    fn allocates_and_writes() {
        let mut mem = Memory::new(ONE_MB).expect("mmap");
        mem.write(0, b"hello").expect("write");
        let mut buf = [0u8; 5];
        mem.read(0, &mut buf).expect("read");
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn rejects_out_of_bounds_write() {
        let mut mem = Memory::new(ONE_MB).expect("mmap");
        assert!(mem.write(ONE_MB - 4, &[1, 2, 3, 4, 5]).is_err());
    }

    #[test]
    fn rejects_overflow_in_bounds_check() {
        let mut mem = Memory::new(ONE_MB).expect("mmap");
        assert!(mem.write(u64::MAX - 1, &[1, 2, 3]).is_err());
    }

    #[test]
    fn write_at_boundary_succeeds() {
        let mut mem = Memory::new(ONE_MB).expect("mmap");
        let buf = vec![0xab; 16];
        mem.write(ONE_MB - 16, &buf).expect("write at boundary");
        let mut readback = [0u8; 16];
        mem.read(ONE_MB - 16, &mut readback).expect("read at boundary");
        assert_eq!(readback.as_slice(), buf.as_slice());
    }
}
