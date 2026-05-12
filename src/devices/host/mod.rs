//! Host-side resource wrappers used by the virtio devices to do real
//! I/O. Each file wraps one Linux kernel facility:
//!
//!   tap.rs           a TAP fd  (used by virtio/net.rs for L2 traffic)
//!   vhost_vsock.rs   /dev/vhost-vsock (used by virtio/vsock.rs to hand
//!                    queue addresses + kick/call eventfds to the kernel)

mod tap;
mod vhost_vsock;

pub(crate) use tap::Tap;
pub(crate) use vhost_vsock::{VhostVsock, VringConfig};
