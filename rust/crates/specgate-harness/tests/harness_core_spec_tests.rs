use std::path::{Path, PathBuf};

use specgate_harness::Harness;
use specgate_types::{CaseStatus, RunError, RunOutcome};

#[test]
fn single_case_pass() {
    let outcome = harness().run_spec(fixture_spec("simple_pass.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, 1);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "basic_case");
    assert_eq!(report.results[0].status, CaseStatus::Pass);
}

#[test]
fn single_case_fail() {
    let outcome = harness().run_spec(fixture_spec("simple_fail.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.total, 1);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "basic_case");
    assert_eq!(report.results[0].status, CaseStatus::Fail);
}

#[test]
fn multiple_cases_mixed() {
    let outcome = harness().run_spec(fixture_spec("multi_mixed.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 1);
    assert_eq!(report.total, 3);
}

#[test]
fn all_cases_pass() {
    let outcome = harness().run_spec(fixture_spec("multi_pass.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 3);
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, 3);
}

#[test]
fn spec_not_found() {
    let outcome = harness().run_spec(fixture_spec("nonexistent.spec.yaml"));

    assert_eq!(
        outcome,
        RunOutcome::Error {
            error: RunError::SpecNotFound {
                path: "fixtures/nonexistent.spec.yaml".to_string(),
            },
        }
    );
}

#[test]
fn spec_invalid_yaml() {
    let outcome = harness().run_spec(fixture_spec("bad_yaml.spec.yaml"));

    match outcome {
        RunOutcome::Error {
            error: RunError::SpecInvalid { detail },
        } => {
            assert!(!detail.is_empty());
        }
        other => panic!("expected SpecInvalid, got {other:?}"),
    }
}

#[test]
fn binding_not_found() {
    let outcome = harness().run_spec(fixture_spec("bad_binding.spec.yaml"));

    assert_eq!(
        outcome,
        RunOutcome::Error {
            error: RunError::BindingNotFound {
                binding: "nonexistent".to_string(),
            },
        }
    );
}

#[test]
fn backend_not_found() {
    let outcome = harness().run_spec(fixture_spec("unknown_lang.spec.yaml"));

    assert_eq!(
        outcome,
        RunOutcome::Error {
            error: RunError::BackendNotFound {
                language: "unknown_lang".to_string(),
            },
        }
    );
}

#[test]
fn generate_fails() {
    let outcome = harness().run_spec(fixture_spec("generate_error.spec.yaml"));

    match outcome {
        RunOutcome::Error {
            error: RunError::GenerateFailed { detail },
        } => {
            assert!(!detail.is_empty());
        }
        other => panic!("expected GenerateFailed, got {other:?}"),
    }
}

#[test]
fn build_fails() {
    let outcome = harness().run_spec(fixture_spec("build_error.spec.yaml"));

    match outcome {
        RunOutcome::Error {
            error: RunError::BuildFailed { detail },
        } => {
            assert!(!detail.is_empty());
        }
        other => panic!("expected BuildFailed, got {other:?}"),
    }
}

#[test]
fn report_includes_metadata() {
    let outcome = harness().run_spec(fixture_spec("simple_pass.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.spec_name, "simple_pass");
    assert_eq!(report.binding, "mock");
    assert!(!report.timestamp.is_empty());
}

#[test]
fn empty_cases_vacuous_pass() {
    let outcome = harness().run_spec(fixture_spec("no_cases.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, 0);
    assert!(report.results.is_empty());
}

#[test]
fn default_binding() {
    let outcome = harness().run_spec(fixture_spec("no_binding.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.binding, "mock");
    assert_eq!(report.passed, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.total, 1);
}

#[test]
fn all_cases_fail() {
    let outcome = harness().run_spec(fixture_spec("all_fail.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 2);
    assert_eq!(report.total, 2);
    assert_eq!(report.results.len(), 2);
    assert!(report.results.iter().all(|result| result.status == CaseStatus::Fail));
}

#[test]
fn binding_invalid_schema() {
    let outcome = harness().run_spec(fixture_spec("bad_binding_schema.spec.yaml"));

    match outcome {
        RunOutcome::Error {
            error: RunError::SpecInvalid { detail },
        } => {
            assert!(detail.contains("bad_schema"));
        }
        other => panic!("expected SpecInvalid, got {other:?}"),
    }
}

#[test]
fn mixed_results_ordering() {
    let outcome = harness().run_spec(fixture_spec("multi_mixed.spec.yaml"));

    let report = expect_complete(outcome);
    let actual = report
        .results
        .iter()
        .map(|result| (&result.name, &result.status))
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (&"alpha_case".to_string(), &CaseStatus::Pass),
            (&"beta_case".to_string(), &CaseStatus::Fail),
            (&"gamma_case".to_string(), &CaseStatus::Pass),
        ]
    );
}

fn harness() -> Harness {
    Harness::new(repo_root())
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
}

fn fixture_spec(file_name: &str) -> PathBuf {
    Path::new("fixtures").join(file_name)
}

fn expect_complete(outcome: RunOutcome) -> specgate_types::RunReport {
    match outcome {
        RunOutcome::Complete { report } => report,
        other => panic!("expected Complete, got {other:?}"),
    }
}
