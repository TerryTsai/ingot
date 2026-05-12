//! Linux x86_64 boot subsystem. `Image` is the in-memory bringup — it
//! borrows guest memory plus the config inputs, lays every fixed-address
//! structure into place via the `Asm` byte assembler, and returns the
//! vCPU register state that jumps into the kernel. `layout` holds every
//! address `Image` writes to. `asm` is the assembler primitive — it
//! knows nothing about Linux or x86 and is the natural seam for future
//! per-architecture boot images.

mod asm;
mod image;
mod layout;

pub(crate) use image::Image;
pub(crate) use layout::TSS_ADDR;
