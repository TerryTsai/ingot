//! Minimal long-mode GDT: null + 64-bit code + 64-bit data.
//!
//! Bases zero, limits ignored in long mode but set to 0xFFFFFFFF for
//! sanity. The L bit on the code descriptor is what puts the CPU into
//! 64-bit submode. Placed at 0x500, well below the 1 MiB BIOS/legacy
//! area; the kernel installs its own GDT early in startup_64.

use crate::error::HostError;
use crate::guest_mem::GuestMem;

pub const GDT_ADDR: u64 = 0x500;
pub const GDT_LIMIT: u16 = 23;
pub const CODE_SELECTOR: u16 = 0x10;
pub const DATA_SELECTOR: u16 = 0x18;

const NULL_DESCRIPTOR: u64 = 0;
const CODE_64BIT: u64 = 0x00AF_9A00_0000_FFFF;
const DATA_64BIT: u64 = 0x00CF_9200_0000_FFFF;

pub fn build(mem: &mut GuestMem) -> Result<(), HostError> {
    write_entry(mem, 0, NULL_DESCRIPTOR)?;
    write_entry(mem, 1, CODE_64BIT)?;
    write_entry(mem, 2, DATA_64BIT)?;
    Ok(())
}

fn write_entry(mem: &mut GuestMem, idx: u64, value: u64) -> Result<(), HostError> {
    mem.write(GDT_ADDR + idx * 8, &value.to_le_bytes())
        .map_err(|_| HostError::InvalidConfig("gdt write failed"))?;
    Ok(())
}
