//! Minimal x86_64 ELF parser for uncompressed vmlinux.
//!
//! Reads the ELF header and `PT_LOAD` program headers, copying each
//! segment to its `p_paddr` in guest memory. BSS (the difference
//! between `p_filesz` and `p_memsz`) is left zero — `GuestMem` is
//! mmap'd anonymous, which is zero-initialized.

use crate::error::HostError;
use crate::guest_mem::GuestMem;

use super::KernelImage;

const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;

const EHDR_SIZE: usize = 64;
const PHDR_SIZE: usize = 56;

pub fn load(image: &[u8], mem: &mut GuestMem) -> Result<KernelImage, HostError> {
    validate_header(image)?;
    let (e_entry, e_phoff, e_phnum) = read_header_fields(image);
    let mut end_addr = 0u64;
    for i in 0..e_phnum {
        let phdr = parse_phdr(image, e_phoff + i * PHDR_SIZE)?;
        if phdr.p_type != PT_LOAD {
            continue;
        }
        load_segment(image, mem, &phdr)?;
        end_addr = end_addr.max(phdr.p_paddr + phdr.p_memsz);
    }
    Ok(KernelImage { entry_point: e_entry, end_addr })
}

struct Phdr {
    p_type: u32,
    p_offset: usize,
    p_paddr: u64,
    p_filesz: usize,
    p_memsz: u64,
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

fn read_header_fields(image: &[u8]) -> (u64, usize, usize) {
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

fn load_segment(image: &[u8], mem: &mut GuestMem, phdr: &Phdr) -> Result<(), HostError> {
    if phdr.p_offset + phdr.p_filesz > image.len() {
        return Err(HostError::InvalidConfig("ELF: PT_LOAD segment past file end"));
    }
    let segment = &image[phdr.p_offset..phdr.p_offset + phdr.p_filesz];
    mem.write(phdr.p_paddr, segment)
        .map_err(|_| HostError::InvalidConfig("guest memory write failed"))?;
    Ok(())
}
