//! Boot subsystem tests. Synthetic-ELF helpers + per-component test files.

mod elf;

pub(super) const ENTRY_POINT: u64 = 0x10_0000;
pub(super) const SEGMENT_PADDR: u64 = 0x10_0000;
pub(super) const MEM_SIZE: u64 = 4 * 1024 * 1024;

/// Build a minimal x86_64 ELF binary in memory: one PT_LOAD segment
/// containing `payload` at physical address `p_paddr`. Returns the
/// raw bytes.
pub(super) fn synth_elf(p_paddr: u64, payload: &[u8]) -> Vec<u8> {
    synth_elf_full(p_paddr, payload, payload.len() as u64)
}

pub(super) fn synth_elf_full(p_paddr: u64, payload: &[u8], p_memsz: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64 + 56 + payload.len());
    buf.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    buf.extend_from_slice(&2u16.to_le_bytes()); // e_type = ET_EXEC
    buf.extend_from_slice(&62u16.to_le_bytes()); // e_machine = EM_X86_64
    buf.extend_from_slice(&1u32.to_le_bytes()); // e_version
    buf.extend_from_slice(&ENTRY_POINT.to_le_bytes()); // e_entry
    buf.extend_from_slice(&64u64.to_le_bytes()); // e_phoff
    buf.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
    buf.extend_from_slice(&0u32.to_le_bytes()); // e_flags
    buf.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize
    buf.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize
    buf.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
    buf.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx
    debug_assert_eq!(buf.len(), 64);
    buf.extend_from_slice(&1u32.to_le_bytes()); // p_type = PT_LOAD
    buf.extend_from_slice(&5u32.to_le_bytes()); // p_flags
    buf.extend_from_slice(&(64u64 + 56).to_le_bytes()); // p_offset = end of phdr
    buf.extend_from_slice(&p_paddr.to_le_bytes()); // p_vaddr
    buf.extend_from_slice(&p_paddr.to_le_bytes()); // p_paddr
    buf.extend_from_slice(&(payload.len() as u64).to_le_bytes()); // p_filesz
    buf.extend_from_slice(&p_memsz.to_le_bytes()); // p_memsz
    buf.extend_from_slice(&0x1000u64.to_le_bytes()); // p_align
    debug_assert_eq!(buf.len(), 64 + 56);
    buf.extend_from_slice(payload);
    buf
}
