// Operation returning Option — None path.
use specgate::*;

#[spec_operation("find", spec = "fixture.option_none")]
pub fn find(items: &[i32], target: i32) -> Option<usize> {
    items.iter().position(|&x| x == target)
}
