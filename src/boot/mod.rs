//! Linux x86_64 boot subsystem. `BootImage` is the one stateful
//! primitive: it borrows guest memory plus the config inputs and lays
//! every fixed-address bringup structure into place, then returns the
//! vCPU register state that jumps into the kernel. `layout` holds
//! every address it writes to.

mod boot_image;
mod layout;

pub(crate) use boot_image::BootImage;
pub(crate) use layout::TSS_ADDR;
