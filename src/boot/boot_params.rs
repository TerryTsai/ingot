//! Linux x86_64 boot_params builder. The full struct is 4 KiB with
//! many nested sub-structs; we operate on a byte buffer at known
//! offsets and only set the fields the 64-bit boot protocol reads.

use crate::error::HostError;
use crate::memory::Memory;

const SIZE: usize = 4096;

const E820_ENTRIES_OFFSET: usize = 0x1E8;
const EXT_RAMDISK_IMAGE: usize = 0x0C0;
const EXT_RAMDISK_SIZE: usize = 0x0C4;
const EXT_CMD_LINE_PTR: usize = 0x0C8;
const HDR_BOOT_FLAG: usize = 0x1FE;
const HDR_HEADER: usize = 0x202;
const HDR_VERSION: usize = 0x206;
const HDR_TYPE_OF_LOADER: usize = 0x210;
const HDR_RAMDISK_IMAGE: usize = 0x218;
const HDR_RAMDISK_SIZE: usize = 0x21C;
const HDR_CMD_LINE_PTR: usize = 0x228;
const HDR_CMDLINE_SIZE: usize = 0x238;
const E820_TABLE_OFFSET: usize = 0x2D0;

const E820_ENTRY_SIZE: usize = 20;
const E820_MAX: u8 = 128;

const TYPE_OF_LOADER_UNDEFINED: u8 = 0xFF;
const BOOT_FLAG: u16 = 0xAA55;
const HEADER_MAGIC: u32 = 0x53726448;
const VERSION_2_15: u16 = 0x020F;

pub const E820_RAM: u32 = 1;
pub const E820_RESERVED: u32 = 2;

pub struct BootParams {
    bytes: [u8; SIZE],
}

impl BootParams {
    pub fn new() -> Self {
        let mut bp = BootParams { bytes: [0; SIZE] };
        bp.bytes[HDR_TYPE_OF_LOADER] = TYPE_OF_LOADER_UNDEFINED;
        bp.put(HDR_BOOT_FLAG, BOOT_FLAG.to_le_bytes());
        bp.put(HDR_HEADER, HEADER_MAGIC.to_le_bytes());
        bp.put(HDR_VERSION, VERSION_2_15.to_le_bytes());
        bp
    }

    pub fn set_cmdline(&mut self, addr: u64, max_size: u32) {
        self.put(HDR_CMD_LINE_PTR, (addr as u32).to_le_bytes());
        self.put(EXT_CMD_LINE_PTR, ((addr >> 32) as u32).to_le_bytes());
        self.put(HDR_CMDLINE_SIZE, max_size.to_le_bytes());
    }

    pub fn set_initramfs(&mut self, addr: u64, size: u32) {
        self.put(HDR_RAMDISK_IMAGE, (addr as u32).to_le_bytes());
        self.put(EXT_RAMDISK_IMAGE, ((addr >> 32) as u32).to_le_bytes());
        self.put(HDR_RAMDISK_SIZE, size.to_le_bytes());
        self.put(EXT_RAMDISK_SIZE, 0u32.to_le_bytes());
    }

    pub fn add_e820_entry(&mut self, addr: u64, size: u64, kind: u32) -> Result<(), HostError> {
        let count = self.bytes[E820_ENTRIES_OFFSET];
        if count >= E820_MAX {
            return Err(HostError::InvalidConfig("too many e820 entries"));
        }
        let off = E820_TABLE_OFFSET + (count as usize) * E820_ENTRY_SIZE;
        self.put(off, addr.to_le_bytes());
        self.put(off + 8, size.to_le_bytes());
        self.put(off + 16, kind.to_le_bytes());
        self.bytes[E820_ENTRIES_OFFSET] = count + 1;
        Ok(())
    }

    pub fn write_to(&self, addr: u64, mem: &mut Memory) -> Result<(), HostError> {
        mem.write(addr, &self.bytes)
            .map_err(|_| HostError::InvalidConfig("boot_params write failed"))?;
        Ok(())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    fn put<const N: usize>(&mut self, offset: usize, bytes: [u8; N]) {
        self.bytes[offset..offset + N].copy_from_slice(&bytes);
    }
}

impl Default for BootParams {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::{build_boot_params, Initramfs};

    fn u16_at(b: &[u8], off: usize) -> u16 { u16::from_le_bytes(b[off..off + 2].try_into().unwrap()) }
    fn u32_at(b: &[u8], off: usize) -> u32 { u32::from_le_bytes(b[off..off + 4].try_into().unwrap()) }
    fn u64_at(b: &[u8], off: usize) -> u64 { u64::from_le_bytes(b[off..off + 8].try_into().unwrap()) }

    #[test]
    fn defaults_set_protocol_signature() {
        let bp = BootParams::new();
        let b = bp.as_bytes();
        assert_eq!(b[0x210], 0xFF);
        assert_eq!(u16_at(b, 0x1FE), 0xAA55);
        assert_eq!(u32_at(b, 0x202), 0x53726448);
        assert_eq!(u16_at(b, 0x206), 0x020F);
    }

    #[test]
    fn cmdline_and_initramfs_fields() {
        let mut bp = BootParams::new();
        bp.set_cmdline(0x20000, 2048);
        bp.set_initramfs(0x500_0000, 0x10_0000);
        let b = bp.as_bytes();
        assert_eq!(u32_at(b, 0x228), 0x20000);
        assert_eq!(u32_at(b, 0x0C8), 0);
        assert_eq!(u32_at(b, 0x238), 2048);
        assert_eq!(u32_at(b, 0x218), 0x500_0000);
        assert_eq!(u32_at(b, 0x0C0), 0);
        assert_eq!(u32_at(b, 0x21C), 0x10_0000);
    }

    #[test]
    fn e820_entries_appended() {
        let mut bp = BootParams::new();
        bp.add_e820_entry(0, 0xA_0000, E820_RAM).unwrap();
        bp.add_e820_entry(0xA_0000, 0x6_0000, E820_RESERVED).unwrap();
        let b = bp.as_bytes();
        assert_eq!(b[0x1E8], 2);
        assert_eq!(u64_at(b, 0x2D0), 0);
        assert_eq!(u64_at(b, 0x2D0 + 8), 0xA_0000);
        assert_eq!(u32_at(b, 0x2D0 + 16), E820_RAM);
    }

    #[test]
    fn build_boot_params_layout() {
        let initramfs = Initramfs { start_addr: 0x500_0000, size: 0x10_0000 };
        let bp = build_boot_params(Some(&initramfs), 0x20000, 2048, 128 * 1024 * 1024).unwrap();
        let b = bp.as_bytes();
        assert_eq!(u32_at(b, 0x218), 0x500_0000);
        assert_eq!(b[0x1E8], 3);
    }
}
