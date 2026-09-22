use specgate::spec_operation;

#[spec_operation("beta", spec = "fixture.multi_toplevel")]
pub fn beta(x: i32) -> i32 {
    x * 2
}

#[test]
fn beta_doubles() {
    assert_eq!(beta(4), 8);
}
