//! Compose `RegState` for direct entry into a long-mode Linux kernel.
//!
//! Writes the page tables and GDT into guest memory, then constructs
//! the kvm_regs and kvm_sregs values that produce a CPU in 64-bit mode
//! with paging enabled, identity-mapped, RIP at the kernel entry, RSI
//! at the boot_params address per the 64-bit boot protocol.

use kvm_bindings::{kvm_regs, kvm_segment, kvm_sregs};

use crate::error::HostError;
use crate::guest_mem::GuestMem;
use crate::kvm::RegState;

use super::gdt::{self, CODE_SELECTOR, DATA_SELECTOR, GDT_ADDR, GDT_LIMIT};
use super::pagetables::{self, PML4_ADDR};
use super::KernelImage;

const CR0_PE: u64 = 1 << 0;
const CR0_NE: u64 = 1 << 5;
const CR0_PG: u64 = 1 << 31;
const CR4_PAE: u64 = 1 << 5;
const EFER_LME: u64 = 1 << 8;
const EFER_LMA: u64 = 1 << 10;

pub fn prime(
    kernel: &KernelImage,
    params_addr: u64,
    mem: &mut GuestMem,
) -> Result<RegState, HostError> {
    pagetables::build(mem)?;
    gdt::build(mem)?;
    Ok(RegState {
        regs: build_regs(kernel.entry_point, params_addr),
        sregs: build_sregs(),
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
    s.cs = code_segment(CODE_SELECTOR);
    s.ds = data_segment(DATA_SELECTOR);
    s.es = data_segment(DATA_SELECTOR);
    s.fs = data_segment(DATA_SELECTOR);
    s.gs = data_segment(DATA_SELECTOR);
    s.ss = data_segment(DATA_SELECTOR);
    setup_system(&mut s);
    s
}

fn setup_system(s: &mut kvm_sregs) {
    // VMX requires TR present with a 64-bit-TSS type; LDTR can be unusable.
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
