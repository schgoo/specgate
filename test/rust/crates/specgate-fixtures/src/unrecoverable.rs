// Operation that panics — unrecoverable outcome.
use specgate_annotations::*;

#[spec_operation("divide")]
fn divide(a: i32, b: i32) -> i32 {
    a / b  // panics on b == 0
}
