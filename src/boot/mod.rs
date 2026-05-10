//! Linux x86_64 boot protocol.

mod boot_params;
mod elf;
mod gdt;
mod initramfs;
mod pagetables;
mod prime;

#[cfg(test)]
mod tests;

use crate::error::HostError;
use crate::guest_mem::GuestMem;
use crate::kvm::RegState;

pub use boot_params::BootParams;

pub struct KernelImage {
    pub entry_point: u64,
    pub end_addr: u64,
}

pub struct InitramfsImage {
    pub start_addr: u64,
    pub size: u64,
}

pub fn load_kernel(image: &[u8], mem: &mut GuestMem) -> Result<KernelImage, HostError> {
    elf::load(image, mem)
}

pub fn load_initramfs(
    image: &[u8],
    mem: &mut GuestMem,
    kernel: &KernelImage,
) -> Result<InitramfsImage, HostError> {
    initramfs::load(image, mem, kernel)
}

pub fn build_boot_params(
    initramfs: Option<&InitramfsImage>,
    cmdline_addr: u64,
    cmdline_max: u32,
    mem_size: u64,
) -> Result<BootParams, HostError> {
    let mut bp = BootParams::new();
    bp.set_cmdline(cmdline_addr, cmdline_max);
    if let Some(img) = initramfs {
        bp.set_initramfs(img.start_addr, img.size as u32);
    }
    bp.add_e820_entry(0, 0xA_0000, boot_params::E820_RAM)?;
    bp.add_e820_entry(0xA_0000, 0x6_0000, boot_params::E820_RESERVED)?;
    bp.add_e820_entry(0x10_0000, mem_size - 0x10_0000, boot_params::E820_RAM)?;
    Ok(bp)
}

pub fn prime_vcpu(
    kernel: &KernelImage,
    params_addr: u64,
    mem: &mut GuestMem,
) -> Result<RegState, HostError> {
    prime::prime(kernel, params_addr, mem)
}
