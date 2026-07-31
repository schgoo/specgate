// Operation returning Option — Some and None paths.
use specgate::*;

#[spec_operation("find", spec = "fixture.option_some")]
pub fn find(items: &[i32], target: i32) -> Option<i32> {
    items.iter().position(|&x| x == target).map(|i| i as i32)
}
