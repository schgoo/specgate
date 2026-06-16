use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as JsonValue;
use specgate_types::{SpecCase, SpecDocument};

fn spec_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist")
        .parent()
        .expect("repo root should exist")
        .join("specs")
        .join("rust.annotations.spec.yaml")
}

fn load_spec() -> SpecDocument {
    let source = fs::read_to_string(spec_path()).expect("spec should be readable");
    serde_yaml::from_str(&source).expect("spec should parse")
}

fn load_case(name: &str) -> SpecCase {
    load_spec()
        .cases
        .into_iter()
        .find(|case| case.name == name)
        .expect("case should exist")
}

fn assert_subset(actual: &JsonValue, expected: &JsonValue) {
    match (actual, expected) {
        (JsonValue::Object(actual), JsonValue::Object(expected)) => {
            for (key, expected_value) in expected {
                let actual_value = actual
                    .get(key)
                    .unwrap_or_else(|| panic!("missing key {key}"));
                assert_subset(actual_value, expected_value);
            }
        }
        (JsonValue::Array(actual), JsonValue::Array(expected)) => {
            assert_eq!(actual.len(), expected.len(), "array length mismatch");
            for (actual_value, expected_value) in actual.iter().zip(expected) {
                assert_subset(actual_value, expected_value);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

fn run_runtime_case(name: &str) {
    let case = load_case(name);
    let source = case
        .inputs
        .get("source")
        .and_then(serde_yaml::Value::as_str)
        .expect("source input should be a string");
    let driver = case
        .inputs
        .get("driver")
        .and_then(serde_yaml::Value::as_str)
        .expect("driver input should be a string");
    let actual =
        annotation_trace_runner::run(name, source, driver).expect("runtime runner should succeed");
    let expected = serde_json::to_value(&case.expected).expect("expected should serialize");
    assert_subset(&actual, &expected);
}

macro_rules! runtime_case {
    ($name:ident) => {
        #[test]
        fn $name() {
            run_runtime_case(stringify!($name));
        }
    };
}

runtime_case!(runtime_capture_stateless);
runtime_case!(runtime_capture_statemachine);
runtime_case!(runtime_checkpoint_attribute);
runtime_case!(runtime_checkpoint_inline);
runtime_case!(runtime_mock_injection);
runtime_case!(runtime_nested_operations);
runtime_case!(runtime_noop_without_feature);
runtime_case!(runtime_multiple_calls);
runtime_case!(runtime_capture_and_checkpoint);
runtime_case!(runtime_field_level_capture);
