// Stateless operation with a return value capture.
use specgate::*;

#[spec_operation("add", spec = "fixture.stateless_add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
