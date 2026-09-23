use specgate::spec_operation;

#[spec_operation("divide", spec = "fixture.unrecoverable")]
pub fn divide(a: i32, b: i32) -> i32 {
    a / b
}

#[test]
fn divide_by_zero_is_a_native_fault() {
    assert!(std::panic::catch_unwind(|| divide(1, 0)).is_err());
}
