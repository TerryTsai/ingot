use super::open_or_skip;

#[test]
fn opens_dev_kvm() {
    let Some(_) = open_or_skip("opens_dev_kvm") else { return; };
}

#[test]
fn creates_vm() {
    let Some(kvm) = open_or_skip("creates_vm") else { return; };
    let _vm = kvm.create_vm().expect("create_vm");
}
