use specgate::spec_operation;

#[spec_operation("run", spec = "fixture.keyword_collision")]
pub fn run(input: &str) -> String {
    format!("executed: {input}")
}

#[test]
fn operation_named_run_remains_unambiguous() {
    assert_eq!(run("job"), "executed: job");
}
