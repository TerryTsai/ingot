//! Linux x86_64 ELF kernel loader. `Kernel::from_path` reads the file
//! and parses every PT_LOAD program header into a `Segment` record —
//! it does NOT touch guest memory. A later `write_to(&mut Memory)`
//! copies each segment to its declared `p_paddr`. The parse/place
//! split lets callers inspect `entry()` / `end()` before deciding
//! how to lay out things downstream of the kernel image.

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
    entry: u64,
    segments: Vec<Segment>,
}

struct Segment {
    paddr: u64,
    bytes: Vec<u8>,
    memsz: u64,
}

impl Kernel {
    /// Read the ELF at `path` and parse every PT_LOAD segment into an
    /// owned `Vec<u8>`. No guest memory is touched here.
    pub fn from_path(path: &Path) -> Result<Self, HostError> {
        let image = std::fs::read(path)
            .map_err(|_| HostError::KernelImageNotReadable(path.to_path_buf()))?;
        Self::validate_header(&image)?;
        let (entry, e_phoff, e_phnum) = Self::header_fields(&image);
        let mut segments = Vec::new();
        for i in 0..e_phnum {
            let phdr = Self::parse_phdr(&image, e_phoff + i * PHDR_SIZE)?;
            if phdr.p_type != PT_LOAD { continue; }
            if phdr.p_offset + phdr.p_filesz > image.len() {
                return Err(HostError::InvalidConfig("ELF: PT_LOAD past file end"));
            }
            let bytes = image[phdr.p_offset..phdr.p_offset + phdr.p_filesz].to_vec();
            segments.push(Segment { paddr: phdr.p_paddr, bytes, memsz: phdr.p_memsz });
        }
        Ok(Kernel { entry, segments })
    }

    pub fn entry(&self) -> u64 { self.entry }

    pub fn end(&self) -> u64 {
        self.segments.iter().map(|s| s.paddr + s.memsz).max().unwrap_or(0)
    }

    /// Copy each parsed segment to its `p_paddr` in guest memory.
    pub fn write_to(&self, mem: &mut Memory) -> Result<(), HostError> {
        for s in &self.segments {
            mem.write(s.paddr, &s.bytes)
                .map_err(|_| HostError::InvalidConfig("kernel segment write failed"))?;
        }
        Ok(())
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

    fn write_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-kernel-{}-{}", name, std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn from_path_parses_entry_and_end_without_touching_memory() {
        let path = write_file("entry", &synth_elf(ENTRY, &[0xf4], 16));
        let kernel = Kernel::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(kernel.entry(), ENTRY);
        assert_eq!(kernel.end(), ENTRY + 16);
        assert_eq!(kernel.segments.len(), 1);
    }

    #[test]
    fn write_to_places_segments() {
        let path = write_file("write", &synth_elf(ENTRY, &[0xf4], 1));
        let kernel = Kernel::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut mem = Memory::new(MEM_SIZE).unwrap();
        kernel.write_to(&mut mem).unwrap();
        let mut buf = [0u8; 1];
        mem.read(ENTRY, &mut buf).unwrap();
        assert_eq!(buf[0], 0xf4);
    }

    #[test]
    fn from_path_rejects_truncated_header() {
        let path = write_file("trunc", &[0x7f, b'E', b'L', b'F']);
        let r = Kernel::from_path(&path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn from_path_rejects_bad_magic() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[0] = 0x00;
        let path = write_file("badmagic", &elf);
        let r = Kernel::from_path(&path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn from_path_rejects_32bit_class() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[4] = 1;
        let path = write_file("32bit", &elf);
        let r = Kernel::from_path(&path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn from_path_rejects_wrong_machine() {
        let mut elf = synth_elf(ENTRY, &[0xf4], 1);
        elf[18..20].copy_from_slice(&3u16.to_le_bytes());
        let path = write_file("wrongmach", &elf);
        let r = Kernel::from_path(&path);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }
}
