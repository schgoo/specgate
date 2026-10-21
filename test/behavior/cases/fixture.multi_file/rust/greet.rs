use specgate::spec_operation;

#[spec_operation("greet", spec = "fixture.multi_file")]
pub fn greet(name: &str) -> String {
    format!("Hello, {name}!")
}

#[test]
fn greets_from_first_source_file() {
    assert_eq!(greet("Ada"), "Hello, Ada!");
}
