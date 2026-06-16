use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use specgate_harness::Harness;
use specgate_rust_backend::RustBackend;
use specgate_types::{CaseStatus, RunError, RunOutcome};

#[test]
fn register_adds_to_backends() {
    let mut harness = harness();

    assert_eq!(
        harness.backend_names(),
        HashSet::from([String::from("mock")])
    );

    harness.register_backend("rust".into(), Arc::new(RustBackend::default()));

    assert_eq!(
        harness.backend_names(),
        HashSet::from([String::from("mock"), String::from("rust")])
    );
}

#[test]
fn register_idempotent() {
    let mut harness = harness();

    assert_eq!(
        harness.backend_names(),
        HashSet::from([String::from("mock")])
    );

    harness.register_backend("rust".into(), Arc::new(RustBackend::default()));
    assert_eq!(
        harness.backend_names(),
        HashSet::from([String::from("mock"), String::from("rust")])
    );

    harness.register_backend("rust".into(), Arc::new(RustBackend::default()));
    assert_eq!(
        harness.backend_names(),
        HashSet::from([String::from("mock"), String::from("rust")])
    );
}

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
fn annotated_spec_produces_traces_file() {
    let _guard = rust_harness_lock();
    let mut harness = harness();
    harness.register_backend("rust".into(), Arc::new(RustBackend::default()));

    let outcome = harness.run_spec(fixture_spec("annotated_with_traces.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 1);
    assert_eq!(report.total, 1);
    assert_eq!(report.results.len(), 1);
    let traces_file = report.results[0]
        .traces_file
        .as_deref()
        .expect("annotated case should produce a traces file");
    assert_eq!(report.results[0].traces_match, Some(true));
    assert_eq!(
        traces_file,
        "target/specgate-harness/traces/increment_once.json"
    );
    let traces_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(traces_file);
    assert!(traces_path.is_file());
    let traces_json = fs::read_to_string(traces_path).expect("traces file should be readable");
    assert!(traces_json.contains("OperationEnter"));
}

#[test]
fn traces_mismatch_reported() {
    let _guard = rust_harness_lock();
    let mut harness = harness();
    harness.register_backend("rust".into(), Arc::new(RustBackend::default()));

    let outcome = harness.run_spec(fixture_spec("annotated_with_wrong_traces.spec.yaml"));

    let report = expect_complete(outcome);
    assert_eq!(report.passed, 0);
    assert_eq!(report.failed, 1);
    assert_eq!(report.total, 1);
    assert_eq!(report.results.len(), 1);
    assert_eq!(report.results[0].name, "increment_once");
    assert_eq!(report.results[0].status, CaseStatus::Fail);
    assert_eq!(report.results[0].traces_match, Some(false));
}

#[test]
fn generated_artifacts_cleaned_up() {
    let repo_root = scratch_repo_root("generated_artifacts_cleaned_up");
    write_cleanup_fixture(&repo_root);

    let report = expect_complete(Harness::new(&repo_root).run_spec(Path::new("cleanup_case.spec.yaml")));
    assert_eq!(report.passed, 1);
    assert_eq!(report.total, 1);
    assert_eq!(report.results[0].status, CaseStatus::Pass);
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
    assert!(
        report
            .results
            .iter()
            .all(|result| result.status == CaseStatus::Fail)
    );
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

fn scratch_repo_root(test_name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("test-scratch")
        .join(format!("{test_name}-{}", unique_suffix()))
}

fn write_cleanup_fixture(repo_root: &Path) {
    let spec_path = repo_root.join("specs").join("cleanup_case.spec.yaml");
    fs::create_dir_all(spec_path.parent().expect("spec parent should exist"))
        .expect("spec directory should be created");
    fs::write(
        &spec_path,
        "name: cleanup_case\nbinding: cleanup\ntarget: mock-target\noutcome: Complete\noutputs:\n  when Complete:\n    report: RunReport\ncases:\n  - name: basic_case\n    desc: Cleanup removes generated test file.\n    expected:\n      outcome: Complete\n    postconditions:\n      - target: assert-file-absent\n        inputs:\n          path: \"{generated_test_path}\"\n        desc: Generated test file does not exist after run\n",
    )
    .expect("cleanup fixture should be written");

    let binding_path = repo_root.join("bindings").join("cleanup.yaml");
    fs::create_dir_all(binding_path.parent().expect("binding parent should exist"))
        .expect("binding directory should be created");
    #[cfg(windows)]
    let binding_content =
        "language: mock\ntargets:\n  assert-file-absent:\n    package_root: .\n    command: 'if exist {path} (exit /b 1) else (exit /b 0)'\n  assert-dir-absent:\n    package_root: .\n    command: 'if exist {path}\\* (exit /b 1) else (exit /b 0)'\n";
    #[cfg(not(windows))]
    let binding_content =
        "language: mock\ntargets:\n  assert-file-absent:\n    package_root: .\n    command: '[ ! -e {path} ]'\n  assert-dir-absent:\n    package_root: .\n    command: '[ ! -d {path} ]'\n";
    fs::write(&binding_path, binding_content).expect("cleanup binding should be written");
}

fn unique_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos()
}

fn rust_harness_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("rust harness lock should not be poisoned")
}
