//! Config validation. Runs once during `Runtime::new`; never panics.

use crate::config::Config;
use crate::error::HostError;

const MIN_MEMORY: u64 = 16 * 1024 * 1024;
const PAGE_SIZE: u64 = 0x1000;
const CMDLINE_MAX: usize = 2048;

pub(super) fn validate(config: &Config) -> Result<(), HostError> {
    validate_paths(config)?;
    validate_size(config)?;
    validate_cmdline(&config.cmdline)?;
    Ok(())
}

fn validate_paths(config: &Config) -> Result<(), HostError> {
    if !config.kernel_path.is_file() {
        return Err(HostError::KernelImageNotReadable(config.kernel_path.clone()));
    }
    if let Some(p) = &config.initramfs_path {
        if !p.is_file() {
            return Err(HostError::InitramfsNotReadable(p.clone()));
        }
    }
    Ok(())
}

fn validate_size(config: &Config) -> Result<(), HostError> {
    if config.vcpus != 1 {
        return Err(HostError::InvalidConfig(
            "only single-vCPU is supported in Phase 2 (Phase 6 adds multi-vCPU)",
        ));
    }
    if config.memory_bytes < MIN_MEMORY {
        return Err(HostError::InvalidConfig("memory must be at least 16 MiB"));
    }
    if config.memory_bytes % PAGE_SIZE != 0 {
        return Err(HostError::InvalidConfig("memory must be page-aligned"));
    }
    Ok(())
}

fn validate_cmdline(cmdline: &str) -> Result<(), HostError> {
    if cmdline.len() > CMDLINE_MAX {
        return Err(HostError::InvalidConfig("cmdline too long (max 2048 bytes)"));
    }
    if cmdline.contains('\0') {
        return Err(HostError::InvalidConfig("cmdline contains NUL"));
    }
    Ok(())
}
