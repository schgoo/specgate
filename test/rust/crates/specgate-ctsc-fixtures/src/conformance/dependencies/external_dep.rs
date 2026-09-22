use specgate::spec_operation;

#[spec_operation("parse_yaml_key", spec = "fixture.cross_dep")]
pub fn parse_yaml_key(input: &str, key: &str) -> String {
    let value: serde_yaml::Value = serde_yaml::from_str(input).expect("fixture YAML must parse");
    value[key].as_str().unwrap_or("null").to_string()
}

#[test]
fn external_yaml_dependency_is_part_of_real_build() {
    assert_eq!(parse_yaml_key("name: SpecGate", "name"), "SpecGate");
}
