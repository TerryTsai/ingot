//! Identity-mapped long-mode page tables covering the first 4 GiB
//! using 1 GiB large pages at the PDPT level.
//!
//! Layout per architecture.md §5:
//!   PML4 at 0x9000  (single entry → PDPT)
//!   PDPT at 0xA000  (four 1 GiB entries: 0, 1, 2, 3 GiB)

use crate::error::HostError;
use crate::memory::Memory;

pub const PML4_ADDR: u64 = 0x9000;
pub const PDPT_ADDR: u64 = 0xA000;

const ENTRY_PRESENT: u64 = 1 << 0;
const ENTRY_RW: u64 = 1 << 1;
const ENTRY_PAGE_SIZE: u64 = 1 << 7;
const PAGE_1GIB: u64 = 0x4000_0000;
const PDPT_ENTRIES: usize = 4;

pub(crate) struct PageTables;

impl PageTables {
    pub fn build(mem: &mut Memory) -> Result<(), HostError> {
        Self::write_entry(mem, PML4_ADDR, 0, PDPT_ADDR | ENTRY_PRESENT | ENTRY_RW)?;
        for i in 0..PDPT_ENTRIES {
            let phys = i as u64 * PAGE_1GIB;
            Self::write_entry(mem, PDPT_ADDR, i, phys | ENTRY_PRESENT | ENTRY_RW | ENTRY_PAGE_SIZE)?;
        }
        Ok(())
    }

    fn write_entry(mem: &mut Memory, table: u64, idx: usize, value: u64) -> Result<(), HostError> {
        mem.write(table + (idx as u64) * 8, &value.to_le_bytes())
            .map_err(|_| HostError::InvalidConfig("page-table write failed"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM_SIZE: u64 = 4 * 1024 * 1024;

    fn read_entry(mem: &Memory, table: u64, idx: usize) -> u64 {
        let mut buf = [0u8; 8];
        mem.read(table + (idx as u64) * 8, &mut buf).expect("read");
        u64::from_le_bytes(buf)
    }

    #[test]
    fn pml4_points_to_pdpt() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        PageTables::build(&mut mem).expect("build");
        let entry = read_entry(&mem, PML4_ADDR, 0);
        assert_eq!(entry & !0xFFF, PDPT_ADDR);
        assert_eq!(entry & 0x3, 0x3);
    }

    #[test]
    fn pdpt_identity_maps_1gib_pages() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        PageTables::build(&mut mem).expect("build");
        for i in 0..4usize {
            let entry = read_entry(&mem, PDPT_ADDR, i);
            let expected_phys = (i as u64) * 0x4000_0000;
            assert_eq!(entry & !0x3FFF_FFFF, expected_phys, "PDPT[{}] phys", i);
            assert_eq!(entry & 0x83, 0x83, "PDPT[{}] flags (P+RW+PS)", i);
        }
    }

    #[test]
    fn pdpt_covers_mmio_base() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        PageTables::build(&mut mem).expect("build");
        let entry = read_entry(&mem, PDPT_ADDR, 3);
        assert_eq!(entry & !0x3FFF_FFFF, 0xC000_0000, "MMIO at 0xD000_0000 falls in PDPT[3]");
    }
}
