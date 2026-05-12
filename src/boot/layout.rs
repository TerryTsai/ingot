//! The fixed guest-physical layout that Ingot boots into.
//!
//! Below 1 MiB is the "bringup" region — small fixed-address structures
//! the kernel reads at entry (page tables, GDT, boot_params, cmdline,
//! MP table). At 1 MiB the kernel ELF loads; the initramfs follows it,
//! page-aligned. Above that is RAM up to the configured ceiling. The
//! IOAPIC, LAPIC, and KVM's TSS sit in the high I/O hole below 4 GiB.
//!
//! Every boot primitive imports its target address from here. They do
//! not pick addresses themselves and the orchestrator does not pass
//! addresses through. One source of truth.
//!
//!   0x0000_0500  GDT             (24 B)
//!   0x0000_7000  boot_params     (4 KiB)
//!   0x0000_9000  PML4            (4 KiB, single entry → PDPT)
//!   0x0000_A000  PDPT            (4 KiB, four 1-GiB identity entries)
//!   0x0002_0000  kernel cmdline  (≤ 2 KiB, NUL-terminated)
//!   0x0009_FC00  MP table        (start of EBDA)
//!   0x000A_0000  ─── e820 hole ─── (EBDA + ROM, reserved)
//!   0x0010_0000  kernel ELF      (variable)
//!   …            initramfs       (page-aligned after kernel)
//!   0xFEC0_0000  IOAPIC MMIO
//!   0xFEE0_0000  LAPIC MMIO
//!   0xFFFB_D000  KVM TSS         (3 pages)

use crate::lifecycle::error::HostError;

pub(crate) const GDT_ADDR:         u64 = 0x0000_0500;
pub(crate) const BOOT_PARAMS_ADDR: u64 = 0x0000_7000;
pub(crate) const PML4_ADDR:        u64 = 0x0000_9000;
pub(crate) const PDPT_ADDR:        u64 = 0x0000_A000;
pub(crate) const CMDLINE_ADDR:     u64 = 0x0002_0000;
pub(crate) const CMDLINE_MAX:      usize = 2048;
pub(crate) const EBDA_START:       u64 = 0x0009_FC00;
pub(crate) const LOW_RAM_TOP:      u64 = 0x000A_0000;
pub(crate) const KERNEL_BASE:      u64 = 0x0010_0000;
pub(crate) const LAPIC_ADDR:       u32 = 0xFEE0_0000;
pub(crate) const IOAPIC_ADDR:      u32 = 0xFEC0_0000;
pub(crate) const TSS_ADDR:         u32 = 0xFFFB_D000;

pub(crate) const E820_RAM:      u32 = 1;
pub(crate) const E820_RESERVED: u32 = 2;

pub(crate) struct E820Entry { pub addr: u64, pub size: u64, pub kind: u32 }

/// The guest's RAM map: low RAM up to EBDA, the EBDA + ROM hole reserved,
/// then high RAM from 1 MiB up to `mem_size`. Derived from the same
/// constants the bringup primitives use — they cannot drift.
pub(crate) fn e820_entries(mem_size: u64) -> Result<[E820Entry; 3], HostError> {
    if mem_size <= KERNEL_BASE {
        return Err(HostError::InvalidConfig("memory size must exceed 1 MiB"));
    }
    Ok([
        E820Entry { addr: 0,           size: LOW_RAM_TOP,                kind: E820_RAM },
        E820Entry { addr: LOW_RAM_TOP, size: KERNEL_BASE - LOW_RAM_TOP,  kind: E820_RESERVED },
        E820Entry { addr: KERNEL_BASE, size: mem_size - KERNEL_BASE,     kind: E820_RAM },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e820_three_entries_covering_full_memory() {
        let entries = e820_entries(128 * 1024 * 1024).unwrap();
        assert_eq!(entries[0].kind, E820_RAM);
        assert_eq!(entries[1].kind, E820_RESERVED);
        assert_eq!(entries[2].kind, E820_RAM);
        assert_eq!(entries[0].addr + entries[0].size, entries[1].addr);
        assert_eq!(entries[1].addr + entries[1].size, entries[2].addr);
        assert_eq!(entries[2].addr + entries[2].size, 128 * 1024 * 1024);
    }

    #[test]
    fn rejects_memory_below_one_mib() {
        assert!(e820_entries(KERNEL_BASE).is_err());
        assert!(e820_entries(KERNEL_BASE - 1).is_err());
    }
}
