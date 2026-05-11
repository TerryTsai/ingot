//! Minimal long-mode GDT: null + 64-bit code + 64-bit data.
//!
//! Bases zero, limits ignored in long mode but set to 0xFFFFFFFF for
//! sanity. The L bit on the code descriptor is what puts the CPU into
//! 64-bit submode. Placed at 0x500; the kernel installs its own GDT
//! early in startup_64.

use crate::error::HostError;
use crate::memory::Memory;

pub const GDT_ADDR: u64 = 0x500;
pub const GDT_LIMIT: u16 = 23;
pub const CODE_SELECTOR: u16 = 0x10;
pub const DATA_SELECTOR: u16 = 0x18;

const NULL_DESCRIPTOR: u64 = 0;
const CODE_64BIT: u64 = 0x00AF_9A00_0000_FFFF;
const DATA_64BIT: u64 = 0x00CF_9200_0000_FFFF;

pub(crate) struct Gdt;

impl Gdt {
    pub fn build(mem: &mut Memory) -> Result<(), HostError> {
        Self::write_entry(mem, 0, NULL_DESCRIPTOR)?;
        Self::write_entry(mem, 1, CODE_64BIT)?;
        Self::write_entry(mem, 2, DATA_64BIT)?;
        Ok(())
    }

    fn write_entry(mem: &mut Memory, idx: u64, value: u64) -> Result<(), HostError> {
        mem.write(GDT_ADDR + idx * 8, &value.to_le_bytes())
            .map_err(|_| HostError::InvalidConfig("gdt write failed"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEM_SIZE: u64 = 4 * 1024 * 1024;

    fn descriptor(mem: &Memory, idx: u64) -> u64 {
        let mut buf = [0u8; 8];
        mem.read(GDT_ADDR + idx * 8, &mut buf).expect("read");
        u64::from_le_bytes(buf)
    }

    #[test]
    fn null_descriptor_is_zero() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        Gdt::build(&mut mem).expect("build");
        assert_eq!(descriptor(&mem, 0), 0);
    }

    #[test]
    fn code_descriptor_is_long_mode() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        Gdt::build(&mut mem).expect("build");
        let cs = descriptor(&mem, 1);
        assert_ne!(cs & (1 << 53), 0, "L bit (long mode)");
        assert_ne!(cs & (1 << 47), 0, "P bit (present)");
        assert_ne!(cs & (1 << 43), 0, "executable type");
        assert_ne!(cs & (1 << 44), 0, "S bit");
    }

    #[test]
    fn data_descriptor_present() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        Gdt::build(&mut mem).expect("build");
        let ds = descriptor(&mem, 2);
        assert_ne!(ds & (1 << 47), 0, "P bit");
        assert_ne!(ds & (1 << 44), 0, "S bit");
        assert_eq!(ds & (1 << 43), 0, "type bit clear (data)");
    }
}
