//! Linux x86_64 boot protocol.
//!
//! Phase 2 in progress. ELF loading is implemented; page tables,
//! boot_params, initramfs placement, and register priming follow.

mod elf;

#[cfg(test)]
mod tests;

use crate::error::HostError;
use crate::guest_mem::GuestMem;
use crate::kvm::RegState;

pub struct KernelImage {
    pub entry_point: u64,
    pub end_addr: u64,
}

pub struct InitramfsImage {
    pub start_addr: u64,
    pub size: u64,
}

pub struct BootParams {
    _placeholder: (),
}

pub fn load_kernel(image: &[u8], mem: &mut GuestMem) -> Result<KernelImage, HostError> {
    elf::load(image, mem)
}

pub fn load_initramfs(
    image: &[u8],
    mem: &mut GuestMem,
    kernel: &KernelImage,
) -> Result<InitramfsImage, HostError> {
    unimplemented!()
}

pub fn build_boot_params(
    kernel: &KernelImage,
    initramfs: Option<&InitramfsImage>,
    cmdline: &str,
    mmio_bus_addr: u64,
) -> BootParams {
    unimplemented!()
}

pub fn prime_vcpu(
    kernel: &KernelImage,
    params_addr: u64,
    mem: &mut GuestMem,
) -> Result<RegState, HostError> {
    unimplemented!()
}
