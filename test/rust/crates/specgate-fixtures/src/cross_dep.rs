// Operation that depends on an external crate (serde_yaml).
// This cannot be #[path] included — the generated runner must
// depend on specgate-fixtures as a Cargo dependency.
use specgate_annotations::*;

#[spec_operation("parse_yaml_key")]
pub fn parse_yaml_key(input: &str, key: &str) -> String {
    let value: serde_yaml::Value = serde_yaml::from_str(input).unwrap();
    value[key].as_str().unwrap_or("null").to_string()
}
