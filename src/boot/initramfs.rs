//! Initramfs placement: page-aligned, immediately after the kernel image.

use crate::error::HostError;
use crate::guest_mem::GuestMem;

use super::{InitramfsImage, KernelImage};

const PAGE_MASK: u64 = 0xFFF;

pub fn load(
    image: &[u8],
    mem: &mut GuestMem,
    kernel: &KernelImage,
) -> Result<InitramfsImage, HostError> {
    let start = (kernel.end_addr + PAGE_MASK) & !PAGE_MASK;
    let size = image.len() as u64;
    if start.checked_add(size).map_or(true, |end| end > mem.size()) {
        return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
    }
    mem.write(start, image)
        .map_err(|_| HostError::InvalidConfig("initramfs write failed"))?;
    Ok(InitramfsImage { start_addr: start, size })
}
