// Operation that panics — unrecoverable outcome.
use specgate::*;

#[spec_operation("divide", spec = "fixture.unrecoverable")]
pub fn divide(a: i32, b: i32) -> i32 {
    a / b // panics on b == 0
}
