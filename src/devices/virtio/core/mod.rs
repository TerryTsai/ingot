//! virtio family infrastructure shared by every virtio device:
//!
//!   mmio.rs   the register layout (magic, version, feature bits,
//!             queue selector, interrupt status) exposed at each
//!             virtio device's MMIO range.
//!
//!   queue.rs  the split-virtqueue mechanism (avail ring, used ring,
//!             descriptor chains) every virtio device uses to move
//!             data to and from guest memory.

pub(crate) mod mmio;
pub(crate) mod queue;
