// Operation returning Option — Some and None paths.
use specgate::*;

#[spec_operation("find")]
pub fn find(items: &[i32], target: i32) -> Option<usize> {
    items.iter().position(|&x| x == target)
}
