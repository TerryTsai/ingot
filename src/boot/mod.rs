//! Linux x86_64 boot subsystem. Five concerns, each in its own file or
//! folder:
//!
//!   asm.rs        Byte assembler over guest memory — the primitive
//!                 every byte placement goes through.
//!   layout.rs     Address constants for every bringup-region structure
//!                 + e820 entries derived from them.
//!   loaders/      File-type handlers. Each reads a host file, places
//!                 its bytes at the right guest address, and returns
//!                 a record of where it landed. Kernel ELF, initramfs.
//!   registers.rs  vCPU register-state composition. Builds the
//!                 kvm_regs + kvm_sregs that enter the kernel in long
//!                 mode at its ELF entry.
//!   image.rs      Orchestrator. Phase 1 runs the loaders; phase 2
//!                 lays the bringup tables through `Asm`; the final
//!                 step calls `Registers::for_kernel`. The Vm
//!                 lifecycle invokes one method here: `Image::install`.

mod asm;
mod image;
mod layout;
mod loaders;
mod registers;

pub(crate) use image::Image;
pub(crate) use layout::TSS_ADDR;
