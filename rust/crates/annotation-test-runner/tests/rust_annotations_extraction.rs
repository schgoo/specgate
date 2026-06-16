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

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("spec-tests")
        .join(name);
    if dir.is_dir() {
        fs::remove_dir_all(&dir).expect("old scratch directory should be removable");
    }
    fs::create_dir_all(&dir).expect("scratch directory should be creatable");
    dir
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

fn run_extraction_case(name: &str) {
    let case = load_case(name);
    let source = case
        .inputs
        .get("source")
        .and_then(serde_yaml::Value::as_str)
        .expect("source input should be a string");
    let scratch = scratch_dir(name);
    let actual = annotation_test_runner::run(Some(name), source, None, &scratch)
        .expect("runner should return JSON");
    let expected = serde_json::to_value(&case.expected).expect("expected should serialize");
    if let (Some(actual_annotations), Some(expected_annotations)) = (
        actual.get("annotations").and_then(JsonValue::as_array),
        expected.get("annotations").and_then(JsonValue::as_array),
    ) {
        let mut actual_annotations = actual_annotations.clone();
        let mut expected_annotations = expected_annotations.clone();
        actual_annotations.sort_by_key(|value| value.to_string());
        expected_annotations.sort_by_key(|value| value.to_string());
        assert_eq!(actual_annotations, expected_annotations);
    }
    let mut actual_without_annotations = actual.clone();
    let mut expected_without_annotations = expected.clone();
    if let JsonValue::Object(actual) = &mut actual_without_annotations {
        actual.remove("annotations");
    }
    if let JsonValue::Object(expected) = &mut expected_without_annotations {
        expected.remove("annotations");
    }
    assert_subset(&actual_without_annotations, &expected_without_annotations);
    if name == "output_file_location" {
        assert!(
            scratch
                .join("target")
                .join("specgate")
                .join("annotations.json")
                .is_file(),
            "annotation registry should be written"
        );
    }
}

macro_rules! extraction_case {
    ($name:ident) => {
        #[test]
        fn $name() {
            run_extraction_case(stringify!($name));
        }
    };
}

extraction_case!(output_file_location);
extraction_case!(import_path);
extraction_case!(passthrough_behavior);
extraction_case!(stateless_operation);
extraction_case!(sequence_operation);
extraction_case!(setup_constructor);
extraction_case!(setup_free_function);
extraction_case!(checkpoint);
extraction_case!(state_on_field);
extraction_case!(setup_environment);
extraction_case!(setup_no_params);
extraction_case!(multiple_setups_same_operation);
extraction_case!(mock);
extraction_case!(full_operation);
extraction_case!(full_statemachine);
extraction_case!(full_stateless);
extraction_case!(full_errormap);
extraction_case!(two_operations);
extraction_case!(no_annotations);
extraction_case!(all_operation_kinds);
extraction_case!(nested_module_symbols);
extraction_case!(async_function);
extraction_case!(generic_function);
extraction_case!(duplicate_operation_name);
extraction_case!(other_attributes_ignored);
extraction_case!(invalid_kind);
extraction_case!(missing_operation_name);
extraction_case!(setup_missing_name);
extraction_case!(setup_with_self);
extraction_case!(mock_missing_name);
extraction_case!(missing_kind);
extraction_case!(state_on_function);
extraction_case!(checkpoint_on_field);
extraction_case!(capture_on_struct);
extraction_case!(capture_on_struct_skips_private);
extraction_case!(capture_struct_and_field_conflict);
extraction_case!(capture_on_stateless_return);
extraction_case!(checkpoint_inline);
extraction_case!(checkpoint_inline_multiple);
extraction_case!(checkpoint_attribute_and_inline);
