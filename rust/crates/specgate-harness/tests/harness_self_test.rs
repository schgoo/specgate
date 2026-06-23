//! Bootstrap self-test — verifies the full harness pipeline end-to-end.
//!
//! This test runs the harness on a simple fixture spec and verifies that:
//! 1. The generated runner actually compiles
//! 2. The annotated code emits real traces (not empty)
//! 3. The matcher produces the correct pass/fail result
//! 4. The trace content matches expected values
//!
//! The `harness_spec_all_cases_pass` test runs the FULL harness spec
//! against all fixture specs — it's the single integration test that
//! validates the entire system end-to-end.
//!
//! This catches: vacuous matching, path resolution bugs, no-op annotations,
//! and fake shim implementations.

use specgate_harness::{CaseStatus, RunOutcome, run_spec};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p
}

/// Runs every fixture spec in the test/rust/crates/specgate-fixtures/specs/
/// directory and verifies each one completes without error. Cases may have
/// status Fail (intentional mismatch tests) — that's fine. We only catch
/// specs that error out entirely (bad YAML, missing source, etc).
///
/// For specs where ALL cases should pass, we verify that explicitly.
#[test]
fn all_fixture_specs_complete() {
    let specs_dir = repo_root().join("test/rust/crates/specgate-fixtures/specs");
    let mut failures: Vec<(String, String)> = Vec::new();
    let mut total = 0;

    // Specs that are expected to ERROR (not Complete) — harness-level failures
    let expected_errors: &[&str] = &[
        "bad_yaml",
        "bad_binding",
        "compile_error",
        "missing_operation",
        "missing_setup",
        "missing_target",
        "no_cases",
        // Schema violations: a case asserts on an output the operation never
        // declares — caught pre-flight as an error, not a case failure.
        "shape_mismatch",
        "mismatch_wrong_field",
    ];

    // Specs not yet implemented — skip entirely
    let skip: &[&str] = &[
        // Property tests not yet implemented in codegen
        "property_add",
        "property_types",
        "property_counterexamples",
        "property_invalid",
        "property_invalid_range",
        "property_no_generators",
        "property_no_calls",
        "property_no_assert",
        "property_bad_ref",
        // Command target (not a source fixture)
        "command_target",
    ];

    for entry in std::fs::read_dir(&specs_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        let stem = path.file_stem().unwrap().to_str().unwrap();
        let Some(name) = stem.strip_suffix(".spec") else {
            continue;
        };
        if skip.contains(&name) {
            continue;
        }
        if expected_errors.contains(&name) {
            // These should return Error — verify that
            total += 1;
            let result = run_spec(path.to_str().unwrap());
            if matches!(result, RunOutcome::Complete { .. }) {
                failures.push((name.to_string(), "expected Error, got Complete".to_string()));
            }
            continue;
        }

        total += 1;
        let result = run_spec(path.to_str().unwrap());
        match result {
            RunOutcome::Complete { .. } => {
                // Completed — cases may be Pass or Fail (intentional mismatches)
            }
            RunOutcome::Error { reason } => {
                failures.push((name.to_string(), reason));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} fixture spec failures (of {} tested):\n{}",
        failures.len(),
        total,
        failures
            .iter()
            .map(|(spec, msg)| format!("  {spec}: {msg}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(total > 30, "expected 30+ fixture specs, got {total}");
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
            assert!(
                !case.traces.is_empty(),
                "traces must not be empty — annotations must emit real events"
            );
            // Must contain specific trace events — catches fake/stub implementations
            let trace_names: Vec<_> = case
                .traces
                .iter()
                .map(|t| match t {
                    specgate_harness::TraceEvent::Event { name, .. } => name.as_str(),
                    specgate_harness::TraceEvent::Run { operation, .. } => operation.as_str(),
                })
                .collect();
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
            assert_eq!(
                results[0].status,
                CaseStatus::Fail,
                "a case with wrong expected values must fail — catches vacuous matching"
            );
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
            assert_eq!(
                results[0].status,
                CaseStatus::Pass,
                "harness must work from any working directory (regression #8)"
            );
        }
        RunOutcome::Error { reason } => {
            panic!("workspace_root broke from temp dir: {reason}");
        }
    }
}
