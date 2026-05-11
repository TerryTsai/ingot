//! Loads an uncompressed x86_64 ELF kernel into guest memory.
//! Reads the ELF header and `PT_LOAD` program headers, copying each
//! segment to its `p_paddr`. BSS is left zero — anonymous mmap is
//! zero-initialized.

use crate::error::HostError;
use crate::memory::Memory;

use super::Kernel;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

pub(crate) struct ElfLoader;

impl ElfLoader {
    pub fn load(image: &[u8], mem: &mut Memory) -> Result<Kernel, HostError> {
        Self::validate_header(image)?;
        let (e_entry, e_phoff, e_phnum) = Self::header_fields(image);
        let mut end_addr = 0u64;
        for i in 0..e_phnum {
            let phdr = Self::parse_phdr(image, e_phoff + i * PHDR_SIZE)?;
            if phdr.p_type != PT_LOAD {
                continue;
            }
            Self::load_segment(image, mem, &phdr)?;
            end_addr = end_addr.max(phdr.p_paddr + phdr.p_memsz);
        }
        Ok(Kernel { entry_point: e_entry, end_addr })
    }

    fn validate_header(image: &[u8]) -> Result<(), HostError> {
        if image.len() < EHDR_SIZE {
            return Err(HostError::InvalidConfig("ELF: truncated header"));
        }
        if image[0..4] != ELF_MAGIC {
            return Err(HostError::InvalidConfig("ELF: bad magic"));
        }
        if image[4] != ELFCLASS64 {
            return Err(HostError::InvalidConfig("ELF: not 64-bit"));
        }
        if image[5] != ELFDATA2LSB {
            return Err(HostError::InvalidConfig("ELF: not little-endian"));
        }
        let e_type = u16::from_le_bytes([image[16], image[17]]);
        if e_type != ET_EXEC && e_type != ET_DYN {
            return Err(HostError::InvalidConfig("ELF: not executable"));
        }
        let e_machine = u16::from_le_bytes([image[18], image[19]]);
        if e_machine != EM_X86_64 {
            return Err(HostError::InvalidConfig("ELF: not x86_64"));
        }
        Ok(())
    }

    fn header_fields(image: &[u8]) -> (u64, usize, usize) {
        let e_entry = u64::from_le_bytes(image[24..32].try_into().unwrap());
        let e_phoff = u64::from_le_bytes(image[32..40].try_into().unwrap()) as usize;
        let e_phnum = u16::from_le_bytes([image[56], image[57]]) as usize;
        (e_entry, e_phoff, e_phnum)
    }

    fn parse_phdr(image: &[u8], off: usize) -> Result<Phdr, HostError> {
        if off + PHDR_SIZE > image.len() {
            return Err(HostError::InvalidConfig("ELF: program header out of file"));
        }
        Ok(Phdr {
            p_type: u32::from_le_bytes(image[off..off + 4].try_into().unwrap()),
            p_offset: u64::from_le_bytes(image[off + 8..off + 16].try_into().unwrap()) as usize,
            p_paddr: u64::from_le_bytes(image[off + 24..off + 32].try_into().unwrap()),
            p_filesz: u64::from_le_bytes(image[off + 32..off + 40].try_into().unwrap()) as usize,
            p_memsz: u64::from_le_bytes(image[off + 40..off + 48].try_into().unwrap()),
        })
    }

    fn load_segment(image: &[u8], mem: &mut Memory, phdr: &Phdr) -> Result<(), HostError> {
        if phdr.p_offset + phdr.p_filesz > image.len() {
            return Err(HostError::InvalidConfig("ELF: PT_LOAD segment past file end"));
        }
        let segment = &image[phdr.p_offset..phdr.p_offset + phdr.p_filesz];
        mem.write(phdr.p_paddr, segment)
            .map_err(|_| HostError::InvalidConfig("guest memory write failed"))?;
        Ok(())
    }
}

struct Phdr {
    p_type: u32,
    p_offset: usize,
    p_paddr: u64,
    p_filesz: usize,
    p_memsz: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: u64 = 0x10_0000;
    const MEM_SIZE: u64 = 4 * 1024 * 1024;

    fn synth_elf(p_paddr: u64, payload: &[u8], p_memsz: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + 56 + payload.len());
        b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u16.to_le_bytes()); // e_type
        b.extend_from_slice(&62u16.to_le_bytes()); // e_machine
        b.extend_from_slice(&1u32.to_le_bytes()); // e_version
        b.extend_from_slice(&ENTRY.to_le_bytes());
        b.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
        b.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
        b.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        b.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
        b.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
        b.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&1u32.to_le_bytes()); // p_type=PT_LOAD
        b.extend_from_slice(&5u32.to_le_bytes()); // p_flags
        b.extend_from_slice(&(64u64 + 56).to_le_bytes()); // p_offset
        b.extend_from_slice(&p_paddr.to_le_bytes()); // p_vaddr
        b.extend_from_slice(&p_paddr.to_le_bytes()); // p_paddr
        b.extend_from_slice(&(payload.len() as u64).to_le_bytes()); // p_filesz
        b.extend_from_slice(&p_memsz.to_le_bytes());
        b.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
        b.extend_from_slice(payload);
        b
    }

    #[test]
    fn loads_minimal_elf() {
        let elf = synth_elf(ENTRY, &[0xf4], 1);
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let k = ElfLoader::load(&elf, &mut mem).expect("load");
        assert_eq!(k.entry_point, ENTRY);
        assert_eq!(k.end_addr, ENTRY + 1);
        let mut readback = [0u8; 1];
        mem.read(ENTRY, &mut readback).expect("read");
        assert_eq!(readback[0], 0xf4);
    }

    #[test]
    fn bss_zero_filled() {
        let elf = synth_elf(ENTRY, &[0x42], 16);
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let k = ElfLoader::load(&elf, &mut mem).expect("load");
        assert_eq!(k.end_addr, ENTRY + 16);
        let mut readback = [0u8; 16];
        mem.read(ENTRY, &mut readback).expect("read");
        assert_eq!(readback[0], 0x42);
        assert!(readback[1..].iter().all(|&b| b == 0));
    }

    #[test]
    fn rejects_truncated_header() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(&[0x7f, b'E', b'L', b'F'], &mut mem).is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[0] = 0x00;
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(&elf, &mut mem).is_err());
    }

    #[test]
    fn rejects_32bit() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[4] = 1;
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(&elf, &mut mem).is_err());
    }

    #[test]
    fn rejects_wrong_machine() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[18..20].copy_from_slice(&3u16.to_le_bytes());
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(&elf, &mut mem).is_err());
    }

    #[test]
    fn rejects_segment_past_file_end() {
        let elf = synth_elf(ENTRY, &[0xf4], 1);
        let truncated = &elf[..elf.len() - 1];
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(truncated, &mut mem).is_err());
    }

    #[test]
    fn rejects_segment_past_guest_mem() {
        let elf = synth_elf(MEM_SIZE + 0x1000, &[0xf4], 1);
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        assert!(ElfLoader::load(&elf, &mut mem).is_err());
    }
}
