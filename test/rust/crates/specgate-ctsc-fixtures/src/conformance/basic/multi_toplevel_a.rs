use specgate::spec_operation;

#[spec_operation("alpha", spec = "fixture.multi_toplevel")]
pub fn alpha(x: i32) -> i32 {
    x + 1
}

#[test]
fn alpha_increments() {
    assert_eq!(alpha(4), 5);
}
