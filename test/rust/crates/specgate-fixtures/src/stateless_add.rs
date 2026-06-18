// Stateless operation with a return value capture.
use specgate_annotations::*;

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
