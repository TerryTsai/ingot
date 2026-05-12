//! Initramfs loader. Reads the file at `path` and writes its bytes at
//! the first page-aligned address after `kernel_end`. Returns an
//! `Initramfs` record so the orchestrator can fill in the corresponding
//! boot_params fields (HDR_RAMDISK_IMAGE / HDR_RAMDISK_SIZE).

use std::path::Path;

use crate::lifecycle::error::HostError;
use crate::lifecycle::memory::Memory;

const PAGE_MASK: u64 = 0xFFF;

pub(crate) struct Initramfs {
    pub start: u64,
    pub size: u64,
}

impl Initramfs {
    pub fn load(mem: &mut Memory, path: &Path, kernel_end: u64) -> Result<Self, HostError> {
        let bytes = std::fs::read(path)
            .map_err(|_| HostError::InitramfsNotReadable(path.to_path_buf()))?;
        let start = (kernel_end + PAGE_MASK) & !PAGE_MASK;
        let size = bytes.len() as u64;
        if start.checked_add(size).is_none_or(|e| e > mem.size()) {
            return Err(HostError::InvalidConfig("initramfs does not fit in guest memory"));
        }
        mem.write(start, &bytes)
            .map_err(|_| HostError::InvalidConfig("initramfs write failed"))?;
        Ok(Initramfs { start, size })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(name: &str, content: &[u8]) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("ingot-initramfs-{}-{}", name, std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn places_after_kernel_page_aligned() {
        let mut mem = Memory::new(0x10_0000).unwrap();
        let path = write_temp("place", b"some-initramfs-content");
        let i = Initramfs::load(&mut mem, &path, 0x12_345).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(i.start, 0x13_000, "rounded up to next page");
        assert_eq!(i.size, b"some-initramfs-content".len() as u64);
        let mut buf = vec![0u8; i.size as usize];
        mem.read(i.start, &mut buf).unwrap();
        assert_eq!(&buf, b"some-initramfs-content");
    }

    #[test]
    fn rejects_when_too_large_for_memory() {
        let mut mem = Memory::new(0x2_000).unwrap();
        let big = vec![0u8; 0x10_000];
        let path = write_temp("toobig", &big);
        let r = Initramfs::load(&mut mem, &path, 0);
        let _ = std::fs::remove_file(&path);
        assert!(r.is_err());
    }

    #[test]
    fn already_aligned_kernel_end_does_not_round_further() {
        let mut mem = Memory::new(0x10_0000).unwrap();
        let path = write_temp("aligned", b"x");
        let i = Initramfs::load(&mut mem, &path, 0x5_000).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(i.start, 0x5_000);
    }
}
