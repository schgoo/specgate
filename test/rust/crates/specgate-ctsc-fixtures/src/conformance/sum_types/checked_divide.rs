use specgate::spec_operation;

#[spec_operation("checked_divide", spec = "fixture.checked_divide")]
pub fn checked_divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else if b < 0 {
        panic!("negative divisor");
    } else {
        Ok(a / b)
    }
}

#[test]
fn declared_error_and_fault_paths_are_distinct() {
    assert_eq!(checked_divide(12, 3), Ok(4));
    assert_eq!(checked_divide(12, 0), Err("division by zero".to_string()));
    let panic = std::panic::catch_unwind(|| checked_divide(12, -3));
    assert!(panic.is_err());
}
