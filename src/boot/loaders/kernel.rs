//! Linux x86_64 ELF kernel loader. Reads the file at `path`, parses
//! the ELF header + each program header, and copies every PT_LOAD
//! segment to its declared `p_paddr` in guest memory. Returns a
//! `Kernel` record with the entry point and the highest end address
//! across all loaded segments — the orchestrator uses `end` to place
//! the initramfs right after the kernel image.

use std::path::Path;

use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

pub(crate) struct Kernel {
    pub entry: u64,
    pub end: u64,
}

impl Kernel {
    pub fn load(mem: &mut Memory, path: &Path) -> Result<Self, HostError> {
        let bytes = std::fs::read(path)
            .map_err(|_| HostError::KernelImageNotReadable(path.to_path_buf()))?;
        Self::validate_header(&bytes)?;
        let (entry, e_phoff, e_phnum) = Self::header_fields(&bytes);
        let mut end = 0u64;
        for i in 0..e_phnum {
            let phdr = Self::parse_phdr(&bytes, e_phoff + i * PHDR_SIZE)?;
            if phdr.p_type != PT_LOAD { continue; }
            if phdr.p_offset + phdr.p_filesz > bytes.len() {
                return Err(HostError::InvalidConfig("ELF: PT_LOAD past file end"));
            }
            mem.write(phdr.p_paddr, &bytes[phdr.p_offset..phdr.p_offset + phdr.p_filesz])
                .map_err(|_| HostError::InvalidConfig("kernel segment write failed"))?;
            end = end.max(phdr.p_paddr + phdr.p_memsz);
        }
        Ok(Kernel { entry, end })
    }

    fn validate_header(image: &[u8]) -> Result<(), HostError> {
        if image.len() < EHDR_SIZE      { return Err(HostError::InvalidConfig("ELF: truncated header")); }
        if image[0..4] != ELF_MAGIC     { return Err(HostError::InvalidConfig("ELF: bad magic")); }
        if image[4] != ELFCLASS64       { return Err(HostError::InvalidConfig("ELF: not 64-bit")); }
        if image[5] != ELFDATA2LSB      { return Err(HostError::InvalidConfig("ELF: not little-endian")); }
        let e_type = u16::from_le_bytes([image[16], image[17]]);
        if e_type != ET_EXEC && e_type != ET_DYN {
            return Err(HostError::InvalidConfig("ELF: not executable"));
        }
        let e_machine = u16::from_le_bytes([image[18], image[19]]);
        if e_machine != EM_X86_64 { return Err(HostError::InvalidConfig("ELF: not x86_64")); }
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
            p_type:   u32::from_le_bytes(image[off..off + 4].try_into().unwrap()),
            p_offset: u64::from_le_bytes(image[off + 8..off + 16].try_into().unwrap()) as usize,
            p_paddr:  u64::from_le_bytes(image[off + 24..off + 32].try_into().unwrap()),
            p_filesz: u64::from_le_bytes(image[off + 32..off + 40].try_into().unwrap()) as usize,
            p_memsz:  u64::from_le_bytes(image[off + 40..off + 48].try_into().unwrap()),
        })
    }
}

struct Phdr { p_type: u32, p_offset: usize, p_paddr: u64, p_filesz: usize, p_memsz: u64 }

#[cfg(test)]
mod tests {
    use super::*;

    const ENTRY: u64 = 0x10_0000;
    const MEM_SIZE: u64 = 4 * 1024 * 1024;

    fn synth_elf(p_paddr: u64, payload: &[u8], p_memsz: u64) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + 56 + payload.len());
        b.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&62u16.to_le_bytes());
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&ENTRY.to_le_bytes());
        b.extend_from_slice(&64u64.to_le_bytes());
        b.extend_from_slice(&0u64.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&64u16.to_le_bytes());
        b.extend_from_slice(&56u16.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[0u8; 6]);
        b.extend_from_slice(&1u32.to_le_bytes());
        b.extend_from_slice(&5u32.to_le_bytes());
        b.extend_from_slice(&(64u64 + 56).to_le_bytes());
        b.extend_from_slice(&p_paddr.to_le_bytes());
        b.extend_from_slice(&p_paddr.to_le_bytes());
        b.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        b.extend_from_slice(&p_memsz.to_le_bytes());
        b.extend_from_slice(&0x1000u64.to_le_bytes());
        b.extend_from_slice(payload);
        b
    }

    fn write_temp_elf(name: &str, payload: &[u8], memsz: u64) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-kernel-load-{}-{}", name, std::process::id()));
        std::fs::write(&p, synth_elf(ENTRY, payload, memsz)).unwrap();
        p
    }

    fn write_raw_elf(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-kernel-load-{}-{}", name, std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn loads_minimal_elf_returning_entry_and_end() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_temp_elf("min", &[0xf4], 16);
        let k = Kernel::load(&mut mem, &path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(k.entry, ENTRY);
        assert_eq!(k.end, ENTRY + 16);
        let mut buf = [0u8; 1];
        mem.read(ENTRY, &mut buf).unwrap();
        assert_eq!(buf[0], 0xf4);
    }

    #[test]
    fn rejects_truncated_header() {
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let path = write_raw_elf("trunc", &[0x7f, b'E', b'L', b'F']);
        let r = Kernel::load(&mut mem, &path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[0] = 0x00;
        let path = write_raw_elf("badmagic", &elf);
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let r = Kernel::load(&mut mem, &path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_32bit_class() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[4] = 1;
        let path = write_raw_elf("32bit", &elf);
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let r = Kernel::load(&mut mem, &path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_wrong_machine() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[18..20].copy_from_slice(&3u16.to_le_bytes());
        let path = write_raw_elf("wrongmach", &elf);
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let r = Kernel::load(&mut mem, &path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_segment_past_file_end() {
        let elf = synth_elf(ENTRY, &[0xf4], 1);
        let truncated = elf[..elf.len() - 1].to_vec();
        let path = write_raw_elf("truncseg", &truncated);
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        let r = Kernel::load(&mut mem, &path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }
}
