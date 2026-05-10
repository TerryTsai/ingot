//! Identity-mapped 4-level long-mode page tables covering the first 1 GiB
//! using 2 MiB large pages.
//!
//! Layout per architecture.md §5:
//!   PML4 at 0x9000 (single entry → PDPT)
//!   PDPT at 0xA000 (single entry → PD)
//!   PD   at 0xB000 (512 entries × 2 MiB = 1 GiB)
//!
//! 1 GiB is enough for the kernel image and initramfs combined; the
//! kernel installs its own page tables long before it needs more.

use crate::error::HostError;
use crate::guest_mem::GuestMem;

pub const PML4_ADDR: u64 = 0x9000;
pub const PDPT_ADDR: u64 = 0xA000;
pub const PD_ADDR: u64 = 0xB000;

const ENTRY_PRESENT: u64 = 1 << 0;
const ENTRY_RW: u64 = 1 << 1;
const ENTRY_PAGE_SIZE: u64 = 1 << 7;
const PAGE_2MIB: u64 = 0x20_0000;
const PD_ENTRIES: usize = 512;

pub fn build(mem: &mut GuestMem) -> Result<(), HostError> {
    write_entry(mem, PML4_ADDR, 0, PDPT_ADDR | ENTRY_PRESENT | ENTRY_RW)?;
    write_entry(mem, PDPT_ADDR, 0, PD_ADDR | ENTRY_PRESENT | ENTRY_RW)?;
    for i in 0..PD_ENTRIES {
        let phys = i as u64 * PAGE_2MIB;
        write_entry(mem, PD_ADDR, i, phys | ENTRY_PRESENT | ENTRY_RW | ENTRY_PAGE_SIZE)?;
    }
    Ok(())
}

fn write_entry(mem: &mut GuestMem, table: u64, idx: usize, value: u64) -> Result<(), HostError> {
    mem.write(table + (idx as u64) * 8, &value.to_le_bytes())
        .map_err(|_| HostError::InvalidConfig("page-table write failed"))?;
    Ok(())
}
