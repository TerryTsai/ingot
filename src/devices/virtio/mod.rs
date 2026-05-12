//! virtio — paravirtual device family. All three members (net, block,
//! vsock) share the same register layout (core/mmio.rs) and descriptor-
//! ring data path (core/queue.rs). Each device file is the protocol on
//! top of that shared infrastructure plus a hookup to a host resource.
//!
//! Skeleton every virtio device follows:
//!   1. Owns a `VirtioMmio` (the register file).
//!   2. `read/write` forward to it; on a queue kick, drain that queue.
//!   3. Sets INT_USED_RING when servicing; bus picks up the interrupt.

mod block;
pub(crate) mod core;
mod net;
mod vsock;

pub(crate) use block::Block;
pub(crate) use net::Net;
pub(crate) use vsock::Vsock;
