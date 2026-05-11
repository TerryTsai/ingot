//! Initramfs placement: page-aligned, immediately after the kernel image.

use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

use super::Kernel;

const PAGE_MASK: u64 = 0xFFF;

pub struct Initramfs {
    pub start_addr: u64,
    pub size: u64,
}

impl Initramfs {
    pub fn load(image: &[u8], mem: &mut Memory, kernel: &Kernel) -> Result<Self, HostError> {
        let start = (kernel.end_addr + PAGE_MASK) & !PAGE_MASK;
        let size = image.len() as u64;
        if start.checked_add(size).is_none_or(|end| end > mem.size()) {
            return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
        }
        mem.write(start, image)
            .map_err(|_| HostError::InvalidConfig("initramfs write failed"))?;
        Ok(Initramfs { start_addr: start, size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM_SIZE: u64 = 4 * 1024 * 1024;

    #[test]
    fn initramfs_placed_after_kernel_page_aligned() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let kernel = Kernel { entry_point: 0x10_0000, end_addr: 0x10_0100 };
        let img = Initramfs::load(b"hello", &mut mem, &kernel).expect("load");
        assert_eq!(img.start_addr & 0xFFF, 0);
        assert!(img.start_addr >= kernel.end_addr);
        assert_eq!(img.size, 5);
        let mut buf = [0u8; 5];
        mem.read(img.start_addr, &mut buf).expect("read");
        assert_eq!(&buf, b"hello");
    }

    #[test]
    fn rejects_initramfs_too_big_for_guest_memory() {
        let mut mem = Memory::new(0x10_2000).expect("mem");
        let kernel = Kernel { entry_point: 0x10_0000, end_addr: 0x10_1000 };
        let big = vec![0u8; 0x10_0000];
        assert!(Initramfs::load(&big, &mut mem, &kernel).is_err());
    }
}
