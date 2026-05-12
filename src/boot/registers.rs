//! vCPU register state — the kvm_regs + kvm_sregs pair that enters
//! the kernel in long mode at its ELF entry. `Registers` IS the
//! value type (replaces the old `RegState` placeholder in `kvm/`);
//! `for_kernel(entry)` composes one and `apply_to(&Vcpu)` writes it.

use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::kvm::{KvmError, Vcpu};

use super::layout::{BOOT_PARAMS_ADDR, GDT_ADDR, PML4_ADDR};

const CR0_PE: u64 = 1 << 0;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;
/// xAPIC at default address, globally enabled, BSP. Leaving APIC_BASE
/// at 0 disables the in-kernel LAPIC and forces every LAPIC MMIO out
/// to userspace.
const APIC_BASE_BSP_ENABLED: u64 = 0xfee0_0000 | 0x800 | 0x100;
const GDT_LIMIT: u16 = 23;
const CODE_SELECTOR: u16 = 0x10;
const DATA_SELECTOR: u16 = 0x18;

pub(crate) struct Registers {
    pub regs: kvm_regs,
    pub sregs: kvm_sregs,
}

impl Registers {
    /// Compose registers that enter the kernel at `kernel_entry` in
    /// long mode, RSI carrying the boot_params address per the Linux
    /// 64-bit boot protocol, paging on, LAPIC enabled.
    pub fn for_kernel(kernel_entry: u64) -> Self {
        let mut regs: kvm_regs = unsafe { std::mem::zeroed() };
        regs.rip = kernel_entry;
        regs.rsi = BOOT_PARAMS_ADDR;
        regs.rflags = 0x2;

        let mut sregs: kvm_sregs = unsafe { std::mem::zeroed() };
        sregs.cs = code_segment();
        sregs.ds = data_segment();
        sregs.es = data_segment();
        sregs.fs = data_segment();
        sregs.gs = data_segment();
        sregs.ss = data_segment();
        sregs.tr.limit = 0xFFFF;
        sregs.tr.type_ = 0xB;
        sregs.tr.present = 1;
        sregs.ldt.unusable = 1;
        sregs.gdt.base = GDT_ADDR;
        sregs.gdt.limit = GDT_LIMIT;
        sregs.cr0 = CR0_PE | CR0_NE | CR0_PG;
        sregs.cr3 = PML4_ADDR;
        sregs.cr4 = CR4_PAE;
        sregs.efer = EFER_LME | EFER_LMA;
        sregs.apic_base = APIC_BASE_BSP_ENABLED;

        Registers { regs, sregs }
    }

    /// Write `regs` then `sregs` to the vCPU.
    pub fn apply_to(&self, vcpu: &Vcpu) -> Result<(), KvmError> {
        vcpu.set_sregs(&self.sregs)?;
        vcpu.set_regs(&self.regs)?;
        Ok(())
    }
}

fn code_segment() -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF; s.selector = CODE_SELECTOR; s.type_ = 0xA;
    s.present = 1; s.s = 1; s.l = 1; s.g = 1;
    s
}

fn data_segment() -> kvm_segment {
    let mut s: kvm_segment = unsafe { std::mem::zeroed() };
    s.limit = 0xFFFFFFFF; s.selector = DATA_SELECTOR; s.type_ = 0x2;
    s.present = 1; s.s = 1; s.db = 1; s.g = 1;
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const KERNEL_ENTRY: u64 = 0x10_0000;

    #[test]
    fn rip_at_kernel_entry_and_rsi_at_boot_params() {
        let r = Registers::for_kernel(KERNEL_ENTRY);
        assert_eq!(r.regs.rip, KERNEL_ENTRY);
        assert_eq!(r.regs.rsi, BOOT_PARAMS_ADDR);
        assert_eq!(r.regs.rflags & 0x2, 0x2);
    }

    #[test]
    fn long_mode_paging_and_pae() {
        let r = Registers::for_kernel(KERNEL_ENTRY);
        assert_ne!(r.sregs.cr0 & CR0_PE, 0, "protected mode");
        assert_ne!(r.sregs.cr0 & CR0_PG, 0, "paging");
        assert_ne!(r.sregs.cr4 & CR4_PAE, 0, "PAE");
        assert_ne!(r.sregs.efer & EFER_LME, 0, "long mode enabled");
        assert_ne!(r.sregs.efer & EFER_LMA, 0, "long mode active");
        assert_eq!(r.sregs.cr3, PML4_ADDR);
    }

    #[test]
    fn cs_is_64bit_code_segment() {
        let r = Registers::for_kernel(KERNEL_ENTRY);
        assert_eq!(r.sregs.cs.l, 1, "L bit (long mode)");
        assert_eq!(r.sregs.cs.type_, 0xA);
        assert_eq!(r.sregs.cs.present, 1);
        assert_eq!(r.sregs.cs.selector, CODE_SELECTOR);
    }

    #[test]
    fn tr_present_ldt_unusable() {
        let r = Registers::for_kernel(KERNEL_ENTRY);
        assert_eq!(r.sregs.tr.present, 1);
        assert_eq!(r.sregs.tr.type_, 0xB);
        assert_eq!(r.sregs.ldt.unusable, 1);
    }

    #[test]
    fn apic_base_enabled_bsp() {
        let r = Registers::for_kernel(KERNEL_ENTRY);
        assert_eq!(r.sregs.apic_base & 0xfee0_0000, 0xfee0_0000);
        assert_ne!(r.sregs.apic_base & 0x800, 0, "globally enabled");
        assert_ne!(r.sregs.apic_base & 0x100, 0, "BSP");
    }
}
