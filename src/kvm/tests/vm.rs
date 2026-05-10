use super::open_or_skip;

#[test]
fn creates_vcpu() {
    let kvm = match open_or_skip("creates_vcpu") {
        Some(k) => k,
        None => return,
    };
    let vm = kvm.create_vm().expect("create_vm");
    let vcpu = vm.create_vcpu(0).expect("create_vcpu");
    assert_eq!(vcpu.id(), 0);
}
