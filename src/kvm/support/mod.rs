//! Supporting infrastructure for the kvm primitives. Each file is
//! either constants (ioctls) or a value type (cpuid) consumed by the
//! Fd primitives at the top level. Not exposed externally; not part
//! of the primary kvm vocabulary.

pub(super) mod cpuid;
pub(super) mod ioctls;
