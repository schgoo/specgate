// Operation returns Result — Err path.
use specgate_annotations::*;

#[spec_operation("divide")]
pub fn divide(a: i32, b: i32) -> Result<i32, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}
