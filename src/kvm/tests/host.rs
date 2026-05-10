use super::open_or_skip;

#[test]
fn opens_dev_kvm() {
    let _ = match open_or_skip("opens_dev_kvm") {
        Some(k) => k,
        None => return,
    };
}

#[test]
fn creates_vm() {
    let kvm = match open_or_skip("creates_vm") {
        Some(k) => k,
        None => return,
    };
    let _vm = kvm.create_vm().expect("create_vm");
}
