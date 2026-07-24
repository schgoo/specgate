// Operation returns Result — Ok path.
use specgate::*;

#[spec_operation("try_divide", spec = "fixture.result_ok")]
pub fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}
