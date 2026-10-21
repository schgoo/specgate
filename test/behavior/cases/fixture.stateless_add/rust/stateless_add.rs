use specgate::spec_operation;

#[spec_operation("add", spec = "fixture.stateless_add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn add_two_and_three() {
    assert_eq!(add(2, 3), 5);
}
