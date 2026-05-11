//! Minimal MP (Intel MultiProcessor Specification 1.4) configuration
//! table. Placed at the start of EBDA so x86_64 kernels — which scan
//! [0, 0x400), [0x9fc00, 0xa0000), [0xf0000, 0x100000) for the "_MP_"
//! signature — discover LAPIC and IOAPIC without needing ACPI/MADT.

use crate::error::HostError;
use crate::memory::Memory;

const EBDA: u64 = 0x9fc00;
const CONFIG_TABLE_ADDR: u32 = EBDA as u32 + 16;
const LAPIC_ADDR: u32 = 0xfee0_0000;
const IOAPIC_ADDR: u32 = 0xfec0_0000;
const OEM_ID: &[u8; 8] = b"INGOT   ";
const PRODUCT_ID: &[u8; 12] = b"VMM         ";
const CFG_LEN_OFFSET: usize = 4;
const CFG_CHECKSUM_OFFSET: usize = 7;
const CFG_ENTRIES_OFFSET: usize = 34;

pub(crate) struct MpTable;

impl MpTable {
    pub fn write_to(num_cpus: u8, mem: &mut Memory) -> Result<(), HostError> {
        let mut buf = Vec::with_capacity(256);
        Self::write_floating_pointer(&mut buf);
        let cfg = buf.len();
        Self::write_config_header(&mut buf);
        let mut entries: u16 = 0;
        for i in 0..num_cpus { Self::write_processor(&mut buf, i); entries += 1; }
        Self::write_bus(&mut buf); entries += 1;
        Self::write_ioapic(&mut buf); entries += 1;
        Self::finalize(&mut buf, cfg, entries);
        mem.write(EBDA, &buf).map_err(|_| HostError::InvalidConfig("mp_table write failed"))?;
        Ok(())
    }

    fn write_floating_pointer(buf: &mut Vec<u8>) {
        let start = buf.len();
        buf.extend_from_slice(b"_MP_");
        buf.extend_from_slice(&CONFIG_TABLE_ADDR.to_le_bytes());
        buf.extend_from_slice(&[1, 4, 0, 0, 0, 0, 0, 0]);
        buf[start + 10] = checksum(&buf[start..start + 16]);
    }

    fn write_config_header(buf: &mut Vec<u8>) {
        buf.extend_from_slice(b"PCMP");
        buf.extend_from_slice(&[0, 0, 4, 0]);
        buf.extend_from_slice(OEM_ID);
        buf.extend_from_slice(PRODUCT_ID);
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&LAPIC_ADDR.to_le_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0]);
    }

    fn write_processor(buf: &mut Vec<u8>, apic_id: u8) {
        buf.extend_from_slice(&[0, apic_id, 0x14, if apic_id == 0 { 0x03 } else { 0x01 }]);
        buf.extend_from_slice(&0x600u32.to_le_bytes());
        buf.extend_from_slice(&0x0fab_fbffu32.to_le_bytes());
        buf.extend_from_slice(&[0u8; 8]);
    }

    fn write_bus(buf: &mut Vec<u8>) {
        buf.push(1);
        buf.push(0);
        buf.extend_from_slice(b"ISA   ");
    }

    fn write_ioapic(buf: &mut Vec<u8>) {
        buf.extend_from_slice(&[2, 0, 0x11, 0x01]);
        buf.extend_from_slice(&IOAPIC_ADDR.to_le_bytes());
    }

    fn finalize(buf: &mut [u8], cfg: usize, entries: u16) {
        let cfg_len = (buf.len() - cfg) as u16;
        buf[cfg + CFG_LEN_OFFSET..cfg + CFG_LEN_OFFSET + 2].copy_from_slice(&cfg_len.to_le_bytes());
        buf[cfg + CFG_ENTRIES_OFFSET..cfg + CFG_ENTRIES_OFFSET + 2].copy_from_slice(&entries.to_le_bytes());
        buf[cfg + CFG_CHECKSUM_OFFSET] = checksum(&buf[cfg..buf.len()]);
    }
}

fn checksum(bytes: &[u8]) -> u8 {
    let s: u32 = bytes.iter().map(|&b| b as u32).sum();
    (256u32.wrapping_sub(s) & 0xff) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floating_pointer_signature_and_checksum() {
        let mut mem = Memory::new(1024 * 1024).expect("mmap");
        MpTable::write_to(1, &mut mem).expect("write");
        let mut fp = [0u8; 16];
        mem.read(EBDA, &mut fp).unwrap();
        assert_eq!(&fp[..4], b"_MP_");
        assert_eq!(fp.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
    }

    #[test]
    fn config_table_signature_and_checksum() {
        let mut mem = Memory::new(1024 * 1024).expect("mmap");
        MpTable::write_to(1, &mut mem).expect("write");
        let mut hdr = [0u8; 44];
        mem.read(CONFIG_TABLE_ADDR as u64, &mut hdr).unwrap();
        assert_eq!(&hdr[..4], b"PCMP");
        let cfg_len = u16::from_le_bytes([hdr[4], hdr[5]]) as usize;
        let mut full = vec![0u8; cfg_len];
        mem.read(CONFIG_TABLE_ADDR as u64, &mut full).unwrap();
        assert_eq!(full.iter().map(|&b| b as u32).sum::<u32>() % 256, 0);
        assert_eq!(u16::from_le_bytes([full[34], full[35]]), 3);
    }
}
