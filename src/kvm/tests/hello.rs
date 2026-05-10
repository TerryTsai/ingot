//! 4 KiB real-mode hello world: the Phase 1 done-criterion test.
//!
//! Maps a single page containing one `hlt` (0xF4) instruction at guest
//! physical 0x1000, sets CS:IP = 0:0x1000, runs, expects KVM_EXIT_HLT.

use crate::kvm::VcpuExit;

use super::open_or_skip;

const PAGE_SIZE: usize = 4096;
const CODE_GUEST_PHYS: u64 = 0x1000;

#[test]
fn real_mode_hlt() {
    let Some(kvm) = open_or_skip("real_mode_hlt") else { return; };
    let vm = kvm.create_vm().expect("create_vm");

    let page = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            PAGE_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(page, libc::MAP_FAILED, "mmap host page");
    unsafe { *(page as *mut u8) = 0xf4; }

    vm.set_user_memory_region(0, CODE_GUEST_PHYS, page as *mut u8, PAGE_SIZE as u64)
        .expect("set_user_memory_region");

    let mut vcpu = vm.create_vcpu(0).expect("create_vcpu");
    let mut sregs = vcpu.get_sregs().expect("get_sregs");
    sregs.cs.base = 0;
    sregs.cs.selector = 0;
    vcpu.set_sregs(&sregs).expect("set_sregs");

    let mut regs = vcpu.get_regs().expect("get_regs");
    regs.rip = CODE_GUEST_PHYS;
    regs.rflags = 0x2;
    vcpu.set_regs(&regs).expect("set_regs");

    match vcpu.run().expect("run") {
        VcpuExit::Hlt => {}
        other => panic!("expected Hlt, got {:?}", other),
    }

    unsafe { libc::munmap(page, PAGE_SIZE); }
}
