use specgate::spec_operation;

#[spec_operation("farewell", spec = "fixture.multi_file")]
pub fn farewell(name: &str) -> String {
    format!("Goodbye, {name}!")
}

#[test]
fn farewells_from_second_source_file() {
    assert_eq!(farewell("Ada"), "Goodbye, Ada!");
}
