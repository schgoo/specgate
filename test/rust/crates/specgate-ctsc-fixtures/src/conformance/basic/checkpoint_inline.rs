use specgate::spec_operation;

#[spec_operation("after_upper", spec = "fixture.checkpoint_inline")]
pub fn after_upper(value: &str) -> String {
    value.to_string()
}

#[spec_operation("process", spec = "fixture.checkpoint_inline")]
pub fn process(data: &str) -> String {
    let upper = data.to_uppercase();
    after_upper(&upper);
    upper.trim().to_string()
}

#[test]
fn records_the_intermediate_uppercase_observation() {
    assert_eq!(process("  hello  "), "HELLO");
}
