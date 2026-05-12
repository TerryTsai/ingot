//! Linux x86_64 boot subsystem. `install` is the verb — Vm calls it
//! to load the kernel + initramfs, build the bringup tables in guest
//! memory, and compose the vCPU register state. There is no "image"
//! noun; boot is a sequence of operations, not a thing.
//!
//!   asm.rs         Byte assembler over guest memory — the primitive
//!                  every byte placement goes through.
//!   layout.rs      Address constants for every bringup-region
//!                  structure + e820 entries derived from them.
//!   loaders/       Owned-payload value types.
//!     kernel.rs    Kernel owns parsed ELF segments; write_to places.
//!     initramfs.rs Initramfs owns file bytes; write_to places.
//!   bringup.rs     Phase-2 byte assembly — free function `write`.
//!   registers.rs   Registers value type + apply_to(Vcpu).
//!   mod.rs         `boot::install` — the verb that composes them all.

mod asm;
mod bringup;
mod layout;
mod loaders;
mod registers;

use std::path::Path;

use crate::lifecycle::error::HostError;
use crate::lifecycle::support::memory::Memory;

use loaders::{Initramfs, Kernel};

pub(crate) use layout::TSS_ADDR;
pub(crate) use registers::Registers;

/// Load the kernel + initramfs, write all bringup tables, return the
/// vCPU register state. Vm applies the returned `Registers` and the
/// guest is ready to run.
pub(crate) fn install(
    mem: &mut Memory,
    kernel_path: &Path,
    initramfs_path: Option<&Path>,
    cmdline: &str,
    vcpus: u8,
    mem_size: u64,
) -> Result<Registers, HostError> {
    let kernel = Kernel::from_path(kernel_path)?;
    let mut initramfs = initramfs_path.map(Initramfs::from_path).transpose()?;

    kernel.write_to(mem)?;
    if let Some(i) = &mut initramfs {
        i.write_to(mem, kernel.end())?;
    }
    bringup::write(mem, cmdline, vcpus, initramfs.as_ref(), mem_size)?;

    Ok(Registers::for_kernel(kernel.entry()))
}
