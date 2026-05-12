//! Initramfs loader. `Initramfs::from_path` reads the file and holds
//! the bytes; `write_to(mem, kernel_end)` chooses the first page-
//! aligned address ≥ `kernel_end`, bounds-checks against guest memory
//! size, writes the bytes, and records the placement. After that
//! `start()` and `size()` return the values boot_params needs.

use std::path::Path;

use crate::lifecycle::error::HostError;
use crate::lifecycle::support::memory::Memory;

const PAGE_MASK: u64 = 0xFFF;

pub(crate) struct Initramfs {
    bytes: Vec<u8>,
    start: u64,
}

impl Initramfs {
    /// Read the file. Guest memory is not touched here.
    pub fn from_path(path: &Path) -> Result<Self, HostError> {
        let bytes = std::fs::read(path)
            .map_err(|_| HostError::InitramfsNotReadable(path.to_path_buf()))?;
        Ok(Initramfs { bytes, start: 0 })
    }

    pub fn size(&self) -> u64 { self.bytes.len() as u64 }
    pub fn start(&self) -> u64 { self.start }

    /// Place at the first page-aligned address ≥ `kernel_end`. Records
    /// `start` so subsequent `start()` calls return where it went.
    pub fn write_to(&mut self, mem: &mut Memory, kernel_end: u64) -> Result<(), HostError> {
        let start = (kernel_end + PAGE_MASK) & !PAGE_MASK;
        let size = self.bytes.len() as u64;
        if start.checked_add(size).is_none_or(|e| e > mem.size()) {
            return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
        }
        mem.write(start, &self.bytes)
            .map_err(|_| HostError::InvalidConfig("initramfs write failed"))?;
        self.start = start;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(name: &str, content: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-initramfs-{}-{}", name, std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn from_path_holds_bytes_without_touching_memory() {
        let path = write_file("hold", b"abc");
        let i = Initramfs::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(i.size(), 3);
        assert_eq!(i.start(), 0, "start unset until write_to");
    }

    #[test]
    fn write_to_places_after_kernel_page_aligned() {
        let path = write_file("place", b"some-initramfs-content");
        let mut i = Initramfs::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut mem = Memory::new(0x10_0000).unwrap();
        i.write_to(&mut mem, 0x12_345).unwrap();
        assert_eq!(i.start(), 0x13_000);
        let mut buf = vec![0u8; i.size() as usize];
        mem.read(i.start(), &mut buf).unwrap();
        assert_eq!(&buf, b"some-initramfs-content");
    }

    #[test]
    fn write_to_rejects_when_too_large() {
        let path = write_file("toobig", &vec![0u8; 0x10_000]);
        let mut i = Initramfs::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut mem = Memory::new(0x2_000).unwrap();
        assert!(i.write_to(&mut mem, 0).is_err());
    }

    #[test]
    fn write_to_aligned_kernel_end_does_not_round_further() {
        let path = write_file("aligned", b"x");
        let mut i = Initramfs::from_path(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let mut mem = Memory::new(0x10_0000).unwrap();
        i.write_to(&mut mem, 0x5_000).unwrap();
        assert_eq!(i.start(), 0x5_000);
    }
}
