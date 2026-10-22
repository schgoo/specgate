use specgate::spec_operation;

#[spec_operation("add", spec = "fixture.property_add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn fixed_inputs_witness_commutativity() {
    assert_eq!(add(17, -4), add(-4, 17));
}
