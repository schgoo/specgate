//! Bootstrap self-test — verifies the full harness pipeline end-to-end.
//!
//! This test runs the harness on a simple fixture spec and verifies that:
//! 1. The generated runner actually compiles
//! 2. The annotated code emits real traces (not empty)
//! 3. The matcher produces the correct pass/fail result
//! 4. The trace content matches expected values
//!
//! This catches: vacuous matching, path resolution bugs, no-op annotations,
//! and fake shim implementations.

use specgate_harness::{run_spec, CaseStatus, RunOutcome};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p
}

#[test]
fn self_test_stateless_add_produces_real_traces() {
    let spec = repo_root().join("test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml");
    let result = run_spec(spec.to_str().unwrap());
    match result {
        RunOutcome::Complete { results } => {
            assert_eq!(results.len(), 1, "stateless_add has exactly 1 case");
            let case = &results[0];
            assert_eq!(case.name, "add_2_3");
            assert_eq!(case.status, CaseStatus::Pass);
            // Traces must be non-empty — catches no-op annotations
            assert!(!case.traces.is_empty(), "traces must not be empty — annotations must emit real events");
            // Must contain specific trace events — catches fake/stub implementations
            let trace_names: Vec<_> = case.traces.iter().map(|t| match t {
                specgate_harness::TraceEvent::Event { name, .. } => name.as_str(),
                specgate_harness::TraceEvent::Run { operation, .. } => operation.as_str(),
            }).collect();
            assert!(trace_names.contains(&"add"), "must contain Run event for 'add'");
            assert!(trace_names.contains(&"$result"), "must contain Event for '$result'");
        }
        RunOutcome::Error { reason } => {
            panic!("self-test failed: {reason}");
        }
    }
}

#[test]
fn self_test_failing_spec_actually_fails() {
    let spec = repo_root().join("test/rust/crates/specgate-fixtures/specs/statemachine_counter_wrong.spec.yaml");
    let result = run_spec(spec.to_str().unwrap());
    match result {
        RunOutcome::Complete { results } => {
            assert_eq!(results.len(), 1);
            assert_eq!(results[0].status, CaseStatus::Fail, 
                "a case with wrong expected values must fail — catches vacuous matching");
        }
        RunOutcome::Error { reason } => {
            panic!("self-test failed with error (should have been Complete with fail): {reason}");
        }
    }
}

#[test]
fn self_test_error_case_returns_error() {
    let spec = repo_root().join("test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml");
    let result = run_spec(spec.to_str().unwrap());
    match result {
        RunOutcome::Error { reason } => {
            assert_eq!(reason, "spec file is not valid YAML");
        }
        RunOutcome::Complete { .. } => {
            panic!("bad YAML should produce Error, not Complete");
        }
    }
}

#[test]
fn workspace_root_is_portable() {
    // Regression test for #8: workspace_root() must resolve to a real
    // directory containing the specgate crates, regardless of the runtime
    // working directory. If this breaks, the generated runner can't find
    // specgate-annotations.
    let spec = repo_root().join("test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml");

    // Change working directory to something unrelated
    let original_dir = std::env::current_dir().unwrap();
    std::env::set_current_dir(std::env::temp_dir()).unwrap();

    let result = run_spec(spec.to_str().unwrap());

    // Restore working directory
    std::env::set_current_dir(original_dir).unwrap();

    match result {
        RunOutcome::Complete { results } => {
            assert_eq!(results[0].status, CaseStatus::Pass,
                "harness must work from any working directory (regression #8)");
        }
        RunOutcome::Error { reason } => {
            panic!("workspace_root broke from temp dir: {reason}");
        }
    }
}
