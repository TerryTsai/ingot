//! File-type handlers — each loader turns a host file into bytes
//! placed at the right address in guest memory and returns a small
//! record describing what landed where. Phase-1 prelude of `Image::install`
//! calls these before phase 2 builds the bringup tables.
//!
//!   kernel.rs     ELF parsing + per-segment placement at p_paddr
//!   initramfs.rs  raw bytes placed page-aligned after the kernel

mod initramfs;
mod kernel;

pub(crate) use initramfs::Initramfs;
pub(crate) use kernel::Kernel;
