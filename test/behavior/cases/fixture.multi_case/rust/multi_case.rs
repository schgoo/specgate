use specgate::spec_operation;

#[spec_operation("add", spec = "fixture.multi_case")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn adds_two_positive_values() {
    assert_eq!(add(2, 3), 5);
}

#[test]
fn adds_negative_and_positive_values() {
    assert_eq!(add(-8, 3), -5);
}
