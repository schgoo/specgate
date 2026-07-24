// Multiple test cases in one spec — two different inputs to the same operation.
use specgate::*;

#[spec_operation("add", spec = "fixture.multi_case")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
