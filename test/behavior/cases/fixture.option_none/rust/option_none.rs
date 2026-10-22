use specgate::spec_operation;

#[spec_operation("find", spec = "fixture.option_none")]
pub fn find(items: Vec<i32>, target: i32) -> Option<i32> {
    items
        .iter()
        .position(|item| *item == target)
        .and_then(|index| i32::try_from(index).ok())
}

#[test]
fn absent_item_returns_none() {
    assert_eq!(find(vec![1, 2, 3], 8), None);
}
