//! Composes `RegState` for direct entry into a long-mode Linux kernel.
//! Writes the page tables and GDT into guest memory, then constructs
//! the kvm_regs and kvm_sregs values that produce a CPU in 64-bit mode
//! with paging enabled, identity-mapped, RIP at the kernel entry, RSI
//! at the boot_params address per the 64-bit boot protocol.

use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::error::HostError;
use crate::kvm::RegState;
use crate::memory::Memory;

use super::gdt::{Gdt, CODE_SELECTOR, DATA_SELECTOR, GDT_ADDR, GDT_LIMIT};
use super::page_tables::{PageTables, PML4_ADDR};
use super::Kernel;

const CR0_PE: u64 = 1 << 0;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
/// MSR_IA32_APIC_BASE: xAPIC at default address, globally enabled, BSP.
/// `KVM_SET_SREGS` writes this MSR; leaving it 0 disables the in-kernel
/// LAPIC and forces every LAPIC MMIO access out to userspace.
const APIC_BASE_BSP_ENABLED: u64 = 0xfee0_0000 | 0x800 | 0x100;

pub(crate) struct VcpuPrimer;

impl VcpuPrimer {
    pub fn prime(kernel: &Kernel, params_addr: u64, mem: &mut Memory) -> Result<RegState, HostError> {
        PageTables::build(mem)?;
        Gdt::build(mem)?;
        Ok(RegState {
            regs: Self::build_regs(kernel.entry_point, params_addr),
            sregs: Self::build_sregs(),
        })
    }

    fn build_regs(entry: u64, params_addr: u64) -> kvm_regs {
        let mut r: kvm_regs = unsafe { std::mem::zeroed() };
        r.rip = entry;
        r.rsi = params_addr;
        r.rflags = 0x2;
        r
    }

    fn build_sregs() -> kvm_sregs {
        let mut s: kvm_sregs = unsafe { std::mem::zeroed() };
        s.cs = Self::code_segment(CODE_SELECTOR);
        s.ds = Self::data_segment(DATA_SELECTOR);
        s.es = Self::data_segment(DATA_SELECTOR);
        s.fs = Self::data_segment(DATA_SELECTOR);
        s.gs = Self::data_segment(DATA_SELECTOR);
        s.ss = Self::data_segment(DATA_SELECTOR);
        Self::setup_system(&mut s);
        s
    }

    fn setup_system(s: &mut kvm_sregs) {
        s.tr.limit = 0xFFFF;
        s.tr.type_ = 0xB;
        s.tr.present = 1;
        s.ldt.unusable = 1;
        s.gdt.base = GDT_ADDR;
        s.gdt.limit = GDT_LIMIT;
        s.cr0 = CR0_PE | CR0_NE | CR0_PG;
        s.cr3 = PML4_ADDR;
        s.cr4 = CR4_PAE;
        s.efer = EFER_LME | EFER_LMA;
        s.apic_base = APIC_BASE_BSP_ENABLED;
    }

    fn code_segment(selector: u16) -> kvm_segment {
        let mut s: kvm_segment = unsafe { std::mem::zeroed() };
        s.limit = 0xFFFFFFFF;
        s.selector = selector;
        s.type_ = 0xA;
        s.present = 1;
        s.s = 1;
        s.l = 1;
        s.g = 1;
        s
    }

    fn data_segment(selector: u16) -> kvm_segment {
        let mut s: kvm_segment = unsafe { std::mem::zeroed() };
        s.limit = 0xFFFFFFFF;
        s.selector = selector;
        s.type_ = 0x2;
        s.present = 1;
        s.s = 1;
        s.db = 1;
        s.g = 1;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::page_tables::PML4_ADDR;

    const MEM_SIZE: u64 = 4 * 1024 * 1024;
    const PARAMS_ADDR: u64 = 0x7000;
    const KERNEL_ENTRY: u64 = 0x10_0000;

    fn kernel() -> Kernel {
        Kernel { entry_point: KERNEL_ENTRY, end_addr: KERNEL_ENTRY + 0x1000 }
    }

    #[test]
    fn rip_and_rsi_match_inputs() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let state = VcpuPrimer::prime(&kernel(), PARAMS_ADDR, &mut mem).expect("prime");
        assert_eq!(state.regs.rip, KERNEL_ENTRY);
        assert_eq!(state.regs.rsi, PARAMS_ADDR);
        assert_eq!(state.regs.rflags & 0x2, 0x2);
    }

    #[test]
    fn long_mode_control_registers() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let state = VcpuPrimer::prime(&kernel(), PARAMS_ADDR, &mut mem).expect("prime");
        assert_ne!(state.sregs.cr0 & 0x1, 0, "PE");
        assert_ne!(state.sregs.cr0 & (1 << 31), 0, "PG");
        assert_eq!(state.sregs.cr3, PML4_ADDR);
        assert_ne!(state.sregs.cr4 & (1 << 5), 0, "PAE");
        assert_ne!(state.sregs.efer & (1 << 8), 0, "LME");
        assert_ne!(state.sregs.efer & (1 << 10), 0, "LMA");
    }

    #[test]
    fn cs_is_64bit_code_segment() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let state = VcpuPrimer::prime(&kernel(), PARAMS_ADDR, &mut mem).expect("prime");
        assert_eq!(state.sregs.cs.l, 1);
        assert_eq!(state.sregs.cs.type_, 0xA);
        assert_eq!(state.sregs.cs.present, 1);
        assert_eq!(state.sregs.cs.selector, 0x10);
    }

    #[test]
    fn tr_present_ldt_unusable() {
        let mut mem = Memory::new(MEM_SIZE).expect("mem");
        let state = VcpuPrimer::prime(&kernel(), PARAMS_ADDR, &mut mem).expect("prime");
        assert_eq!(state.sregs.tr.present, 1);
        assert_eq!(state.sregs.tr.type_, 0xB);
        assert_eq!(state.sregs.ldt.unusable, 1);
    }
}
