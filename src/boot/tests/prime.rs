use super::super::pagetables::PML4_ADDR;
use crate::boot::{prime_vcpu, KernelImage};
use crate::guest_mem::GuestMem;

use super::MEM_SIZE;

const PARAMS_ADDR: u64 = 0x7000;
const KERNEL_ENTRY: u64 = 0x10_0000;

#[test]
fn rip_and_rsi_match_inputs() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let kernel = KernelImage { entry_point: KERNEL_ENTRY, end_addr: KERNEL_ENTRY + 0x1000 };
    let state = prime_vcpu(&kernel, PARAMS_ADDR, &mut mem).expect("prime");
    assert_eq!(state.regs.rip, KERNEL_ENTRY);
    assert_eq!(state.regs.rsi, PARAMS_ADDR);
    assert_eq!(state.regs.rflags & 0x2, 0x2);
}

#[test]
fn long_mode_control_registers() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let kernel = KernelImage { entry_point: KERNEL_ENTRY, end_addr: KERNEL_ENTRY + 0x1000 };
    let state = prime_vcpu(&kernel, PARAMS_ADDR, &mut mem).expect("prime");
    assert_ne!(state.sregs.cr0 & 0x1, 0, "PE");
    assert_ne!(state.sregs.cr0 & (1 << 31), 0, "PG");
    assert_eq!(state.sregs.cr3, PML4_ADDR);
    assert_ne!(state.sregs.cr4 & (1 << 5), 0, "PAE");
    assert_ne!(state.sregs.efer & (1 << 8), 0, "LME");
    assert_ne!(state.sregs.efer & (1 << 10), 0, "LMA");
}

#[test]
fn cs_is_64bit_code_segment() {
    let mut mem = GuestMem::new(MEM_SIZE).expect("mem");
    let kernel = KernelImage { entry_point: KERNEL_ENTRY, end_addr: KERNEL_ENTRY + 0x1000 };
    let state = prime_vcpu(&kernel, PARAMS_ADDR, &mut mem).expect("prime");
    assert_eq!(state.sregs.cs.l, 1, "L bit");
    assert_eq!(state.sregs.cs.type_, 0xA);
    assert_eq!(state.sregs.cs.present, 1);
    assert_eq!(state.sregs.cs.selector, 0x10);
}
