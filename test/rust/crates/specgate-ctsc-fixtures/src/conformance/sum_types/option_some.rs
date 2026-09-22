use specgate::spec_operation;

#[spec_operation("find", spec = "fixture.option_some")]
pub fn find(items: Vec<i32>, target: i32) -> Option<i32> {
    items
        .iter()
        .position(|item| *item == target)
        .and_then(|index| i32::try_from(index).ok())
}

#[test]
fn present_and_absent_items_cover_both_option_arms() {
    assert_eq!(find(vec![4, 7, 9], 7), Some(1));
    assert_eq!(find(vec![4, 7, 9], 8), None);
}
