// Multiple test cases in one spec — two different inputs to the same operation.
use specgate_annotations::*;

#[spec_operation("add")]
fn add(a: i32, b: i32) -> i32 {
    a + b
}
