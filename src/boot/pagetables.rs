//! Identity-mapped long-mode page tables covering the first 4 GiB
//! using 1 GiB large pages at the PDPT level. This is enough to reach
//! the MMIO bus at 0xD000_0000 alongside kernel + initramfs in low
//! memory; the kernel installs its own page tables soon after entry.
//!
//! Layout per architecture.md §5:
//!   PML4 at 0x9000  (single entry → PDPT)
//!   PDPT at 0xA000  (four 1 GiB entries: 0, 1, 2, 3 GiB)

use crate::error::HostError;
use crate::guest_mem::GuestMem;

pub const PML4_ADDR: u64 = 0x9000;
pub const PDPT_ADDR: u64 = 0xA000;

const ENTRY_PRESENT: u64 = 1 << 0;
const ENTRY_RW: u64 = 1 << 1;
const ENTRY_PAGE_SIZE: u64 = 1 << 7;
const PAGE_1GIB: u64 = 0x4000_0000;
const PDPT_ENTRIES: usize = 4;

pub fn build(mem: &mut GuestMem) -> Result<(), HostError> {
    write_entry(mem, PML4_ADDR, 0, PDPT_ADDR | ENTRY_PRESENT | ENTRY_RW)?;
    for i in 0..PDPT_ENTRIES {
        let phys = i as u64 * PAGE_1GIB;
        write_entry(mem, PDPT_ADDR, i, phys | ENTRY_PRESENT | ENTRY_RW | ENTRY_PAGE_SIZE)?;
    }
    Ok(())
}

fn write_entry(mem: &mut GuestMem, table: u64, idx: usize, value: u64) -> Result<(), HostError> {
    mem.write(table + (idx as u64) * 8, &value.to_le_bytes())
        .map_err(|_| HostError::InvalidConfig("page-table write failed"))?;
    Ok(())
}
