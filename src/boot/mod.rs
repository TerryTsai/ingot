//! Linux x86_64 boot protocol primitives. Each primitive lives in its
//! own file and exposes a small struct/impl surface; the orchestrator
//! composes them in `vm.rs` to bring a guest up to its entry point.

mod boot_params;
mod elf_loader;
mod gdt;
mod initramfs;
mod mp_table;
mod page_tables;
mod vcpu_primer;

pub use boot_params::BootParams;
pub(crate) use elf_loader::ElfLoader;
pub use initramfs::Initramfs;
pub(crate) use mp_table::MpTable;
pub(crate) use vcpu_primer::VcpuPrimer;

pub struct Kernel {
    pub entry_point: u64,
    pub end_addr: u64,
}

pub fn build_boot_params(
    initramfs: Option<&Initramfs>,
    cmdline_addr: u64,
    cmdline_max: usize,
    mem_size: u64,
) -> Result<BootParams, crate::lifecycle::error::HostError> {
    let mut bp = BootParams::new();
    bp.set_cmdline(cmdline_addr, cmdline_max as u32);
    if let Some(img) = initramfs {
        bp.set_initramfs(img.start_addr, img.size as u32);
    }
    bp.add_e820_entry(0, 0xA_0000, boot_params::E820_RAM)?;
    bp.add_e820_entry(0xA_0000, 0x6_0000, boot_params::E820_RESERVED)?;
    bp.add_e820_entry(0x10_0000, mem_size - 0x10_0000, boot_params::E820_RAM)?;
    Ok(bp)
}
