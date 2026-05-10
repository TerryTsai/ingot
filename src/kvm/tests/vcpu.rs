use super::open_or_skip;

#[test]
fn gets_and_sets_regs() {
    let Some(kvm) = open_or_skip("gets_and_sets_regs") else { return; };
    let vm = kvm.create_vm().expect("create_vm");
    let vcpu = vm.create_vcpu(0).expect("create_vcpu");
    let mut regs = vcpu.get_regs().expect("get_regs");
    regs.rip = 0x1000;
    regs.rflags = 0x2;
    vcpu.set_regs(&regs).expect("set_regs");
    let regs2 = vcpu.get_regs().expect("get_regs after set");
    assert_eq!(regs2.rip, 0x1000);
}

#[test]
fn gets_and_sets_sregs() {
    let Some(kvm) = open_or_skip("gets_and_sets_sregs") else { return; };
    let vm = kvm.create_vm().expect("create_vm");
    let vcpu = vm.create_vcpu(0).expect("create_vcpu");
    let mut sregs = vcpu.get_sregs().expect("get_sregs");
    sregs.cs.base = 0;
    sregs.cs.selector = 0;
    vcpu.set_sregs(&sregs).expect("set_sregs");
}
