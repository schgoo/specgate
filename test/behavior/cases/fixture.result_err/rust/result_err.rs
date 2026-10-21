use specgate::spec_operation;

#[spec_operation("try_divide", spec = "fixture.result_err")]
pub fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}

#[test]
fn zero_divisor_returns_declared_error() {
    assert_eq!(divide(10, 0), Err("division by zero".to_string()));
}
