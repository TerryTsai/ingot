use super::super::boot_params::{BootParams, E820_RAM, E820_RESERVED};
use crate::boot::{build_boot_params, InitramfsImage};

fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}
fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

#[test]
fn defaults_set_protocol_signature() {
    let bp = BootParams::new();
    let b = bp.as_bytes();
    assert_eq!(b[0x210], 0xFF, "type_of_loader");
    assert_eq!(u16_at(b, 0x1FE), 0xAA55, "boot_flag");
    assert_eq!(u32_at(b, 0x202), 0x53726448, "HdrS magic");
    assert_eq!(u16_at(b, 0x206), 0x020F, "version 2.15");
}

#[test]
fn cmdline_and_initramfs_fields() {
    let mut bp = BootParams::new();
    bp.set_cmdline(0x20000, 2048);
    bp.set_initramfs(0x500_0000, 0x10_0000);
    let b = bp.as_bytes();
    assert_eq!(u32_at(b, 0x228), 0x20000, "cmd_line_ptr");
    assert_eq!(u32_at(b, 0x0C8), 0, "ext_cmd_line_ptr (high 32)");
    assert_eq!(u32_at(b, 0x238), 2048, "cmdline_size");
    assert_eq!(u32_at(b, 0x218), 0x500_0000, "ramdisk_image");
    assert_eq!(u32_at(b, 0x0C0), 0, "ext_ramdisk_image (high 32)");
    assert_eq!(u32_at(b, 0x21C), 0x10_0000, "ramdisk_size");
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
    assert_eq!(u64_at(b, 0x2D0 + 20), 0xA_0000);
    assert_eq!(u32_at(b, 0x2D0 + 36), E820_RESERVED);
}

#[test]
fn build_boot_params_layout() {
    let initramfs = InitramfsImage { start_addr: 0x500_0000, size: 0x10_0000 };
    let bp = build_boot_params(Some(&initramfs), 0x20000, 2048, 128 * 1024 * 1024).unwrap();
    let b = bp.as_bytes();
    assert_eq!(u32_at(b, 0x218), 0x500_0000);
    assert_eq!(b[0x1E8], 3, "three e820 entries");
}
