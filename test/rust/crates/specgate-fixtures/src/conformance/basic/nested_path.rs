// Exact-basename vehicle for the nested-path fixture.
use specgate::*;

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
