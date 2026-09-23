use specgate::spec_operation;

#[spec_operation("add", spec = "fixture.nested_path")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn nested_source_path_operation_is_capturable() {
    assert_eq!(add(11, 4), 15);
}
