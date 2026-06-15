use super::{MockBackend, case_status};
use crate::backend::DiscoveredCase;
use crate::backend::{Backend, Discovery, GeneratedArtifact};
use specgate_types::{BindingFile, CaseResult, CaseStatus, SpecCase, SpecDocument};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn case_status_uses_mock_status_field() {
    let pass = DiscoveredCase {
        raw_name: "any_name".to_string(),
        mock_status: None,
    };
    assert_eq!(case_status(&pass), CaseStatus::Pass);

    let explicit_pass = DiscoveredCase {
        raw_name: "any_name".to_string(),
        mock_status: Some("pass".to_string()),
    };
    assert_eq!(case_status(&explicit_pass), CaseStatus::Pass);

    let fail = DiscoveredCase {
        raw_name: "any_name".to_string(),
        mock_status: Some("fail".to_string()),
    };
    assert_eq!(case_status(&fail), CaseStatus::Fail);
}

#[test]
fn build_and_discover_reads_mock_result_inputs() {
    let backend = MockBackend;
    let spec = spec_with_cases(vec![
        spec_case("alpha", None),
        spec_case("beta", Some("fail")),
        spec_case("gamma", Some("pass")),
    ]);

    let discovery = backend
        .build_and_discover(&mock_binding(), &spec)
        .expect("discovery should succeed");

    assert_eq!(discovery.cases.len(), 3);
    assert_eq!(discovery.cases[0].raw_name, "alpha");
    assert_eq!(discovery.cases[0].mock_status, None);
    assert_eq!(discovery.cases[1].mock_status.as_deref(), Some("fail"));
    assert_eq!(discovery.cases[2].mock_status.as_deref(), Some("pass"));
}

#[test]
fn build_and_discover_ignores_non_string_mock_result_inputs() {
    let backend = MockBackend;
    let mut case = spec_case("alpha", None);
    case.inputs.insert(
        "mock_result".to_string(),
        serde_yaml::to_value(1).expect("value should serialize"),
    );

    let discovery = backend
        .build_and_discover(&mock_binding(), &spec_with_cases(vec![case]))
        .expect("discovery should succeed");

    assert_eq!(discovery.cases[0].mock_status, None);
}

#[test]
fn generate_writes_case_names_to_generated_file() {
    let backend = MockBackend;
    let workdir = create_scratch_dir("generate_writes_case_names");
    let spec = spec_with_name("plain_spec");
    let discovery = Discovery {
        cases: vec![
            DiscoveredCase {
                raw_name: "first".to_string(),
                mock_status: None,
            },
            DiscoveredCase {
                raw_name: "second".to_string(),
                mock_status: Some("fail".to_string()),
            },
        ],
    };

    let generated = backend
        .generate(&mock_binding(), &spec, &discovery, &workdir)
        .expect("generation should succeed");

    let contents = fs::read_to_string(&generated.generated_test_path)
        .expect("generated file should be readable");
    assert_eq!(contents, "first\nsecond");
}

#[test]
fn generate_returns_error_when_workdir_is_missing() {
    let backend = MockBackend;
    let workdir = scratch_path("generate_missing_workdir");
    let spec = spec_with_name("plain_spec");
    let discovery = Discovery {
        cases: vec![DiscoveredCase {
            raw_name: "first".to_string(),
            mock_status: None,
        }],
    };

    let error = backend
        .generate(&mock_binding(), &spec, &discovery, &workdir)
        .expect_err("generation should fail");

    assert!(matches!(
        error,
        specgate_types::RunError::GenerateFailed { detail }
            if detail.contains("failed to write generated tests")
    ));
}

#[test]
fn run_command_writes_results_json() {
    let backend = MockBackend;
    let workdir = create_scratch_dir("run_command_writes_results");
    let generated = GeneratedArtifact {
        generated_test_path: workdir.join("generated.mock"),
        results_path: workdir.join("results.json"),
        cases: vec![
            DiscoveredCase {
                raw_name: "first".to_string(),
                mock_status: None,
            },
            DiscoveredCase {
                raw_name: "second".to_string(),
                mock_status: Some("fail".to_string()),
            },
        ],
        spec_name: "plain_spec".to_string(),
    };

    backend
        .run_command(&mock_binding(), &generated)
        .expect("run should succeed");

    let results = serde_json::from_str::<Vec<CaseResult>>(
        &fs::read_to_string(&generated.results_path).expect("results should exist"),
    )
    .expect("results json should parse");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].status, CaseStatus::Pass);
    assert_eq!(results[1].status, CaseStatus::Fail);
}

#[test]
fn run_command_writes_empty_results_json_when_no_cases_exist() {
    let backend = MockBackend;
    let workdir = create_scratch_dir("run_command_writes_empty_results");
    let generated = GeneratedArtifact {
        generated_test_path: workdir.join("generated.mock"),
        results_path: workdir.join("results.json"),
        cases: Vec::new(),
        spec_name: "plain_spec".to_string(),
    };

    backend
        .run_command(&mock_binding(), &generated)
        .expect("run should succeed");

    let results = serde_json::from_str::<Vec<CaseResult>>(
        &fs::read_to_string(&generated.results_path).expect("results should exist"),
    )
    .expect("results json should parse");
    assert!(results.is_empty());
}

#[test]
fn run_command_returns_error_when_results_cannot_be_written() {
    let backend = MockBackend;
    let workdir = create_scratch_dir("run_command_write_error");
    let generated = GeneratedArtifact {
        generated_test_path: workdir.join("generated.mock"),
        results_path: workdir.join("missing").join("results.json"),
        cases: vec![DiscoveredCase {
            raw_name: "first".to_string(),
            mock_status: None,
        }],
        spec_name: "plain_spec".to_string(),
    };

    let error = backend
        .run_command(&mock_binding(), &generated)
        .expect_err("run should fail");

    assert!(matches!(
        error,
        specgate_types::RunError::BuildFailed { detail }
            if detail.contains("failed to write results")
    ));
}

#[test]
fn collect_results_returns_error_when_results_are_missing() {
    let backend = MockBackend;
    let generated = GeneratedArtifact {
        generated_test_path: scratch_path("collect_results_missing").join("generated.mock"),
        results_path: scratch_path("collect_results_missing").join("results.json"),
        cases: Vec::new(),
        spec_name: "plain_spec".to_string(),
    };

    let error = backend
        .collect_results(&generated)
        .expect_err("collect should fail");

    assert!(matches!(
        error,
        specgate_types::RunError::BuildFailed { detail }
            if detail.contains("failed to read results")
    ));
}

#[test]
fn collect_results_returns_error_for_invalid_json() {
    let backend = MockBackend;
    let workdir = create_scratch_dir("collect_results_invalid_json");
    let generated = GeneratedArtifact {
        generated_test_path: workdir.join("generated.mock"),
        results_path: workdir.join("results.json"),
        cases: Vec::new(),
        spec_name: "plain_spec".to_string(),
    };
    fs::write(&generated.results_path, "not valid json").expect("invalid json should be written");

    let error = backend
        .collect_results(&generated)
        .expect_err("collect should fail");

    assert!(matches!(
        error,
        specgate_types::RunError::BuildFailed { detail }
            if detail.contains("failed to parse results")
    ));
}

fn mock_binding() -> BindingFile {
    BindingFile {
        language: "mock".to_string(),
        targets: BTreeMap::new(),
    }
}

fn spec_with_name(name: &str) -> SpecDocument {
    SpecDocument {
        name: name.to_string(),
        binding: Some(specgate_types::BindingDecl::Single(
            specgate_types::BindingEntry {
                name: "mock".to_string(),
                target: "test".to_string(),
            },
        )),
        depends_on: Vec::new(),
        state: BTreeMap::new(),
        init: BTreeMap::new(),
        operations: BTreeMap::new(),
        invariants: BTreeMap::new(),
        inputs: BTreeMap::new(),
        types: BTreeMap::new(),
        outcome: serde_yaml::Value::String("Complete".to_string()),
        outputs: BTreeMap::new(),
        cases: Vec::new(),
    }
}

fn spec_with_cases(cases: Vec<SpecCase>) -> SpecDocument {
    let mut spec = spec_with_name("plain_spec");
    spec.cases = cases;
    spec
}

fn spec_case(name: &str, mock_result: Option<&str>) -> SpecCase {
    let mut inputs = BTreeMap::new();
    if let Some(mock_result) = mock_result {
        inputs.insert(
            "mock_result".to_string(),
            serde_yaml::Value::String(mock_result.to_string()),
        );
    }

    SpecCase {
        name: name.to_string(),
        desc: format!("case {name}"),
        inputs,
        expected: BTreeMap::new(),
        steps: Vec::new(),
    }
}

fn create_scratch_dir(test_name: &str) -> PathBuf {
    let path = scratch_path(test_name);
    fs::create_dir_all(&path).expect("scratch dir should be created");
    path
}

fn scratch_path(test_name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-scratch")
        .join(format!("{test_name}-{}", unique_suffix()))
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos()
}
