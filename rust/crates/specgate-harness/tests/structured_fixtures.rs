//! Drives the 7 structured / operator / cross-dep fixture specs through
//! the harness and asserts per-case pass/fail status matches what each
//! spec's case names imply ("_fails" cases should fail, others should pass).

use specgate_harness::{run_spec, CaseResult, CaseStatus, RunOutcome};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.pop();
    p
}

fn run(rel: &str) -> Vec<CaseResult> {
    let p = repo_root().join(rel);
    match run_spec(p.to_str().unwrap()) {
        RunOutcome::Complete { results } => results,
        RunOutcome::Error { reason } => panic!("{rel}: error {reason}"),
    }
}

fn expected_status(name: &str) -> CaseStatus {
    if name.contains("_fails") {
        CaseStatus::Fail
    } else {
        CaseStatus::Pass
    }
}

fn assert_all(rel: &str) {
    let results = run(rel);
    assert!(!results.is_empty(), "{rel}: no cases");
    for c in &results {
        let want = expected_status(&c.name);
        assert_eq!(
            c.status, want,
            "{rel}::{}: want {:?}, got {:?}, expected={:?}, traces={:?}",
            c.name, want, c.status, c.expected, c.traces
        );
    }
}

#[test]
fn fixture_operators() {
    assert_all("test/rust/crates/specgate-fixtures/specs/operators.spec.yaml");
}
#[test]
fn fixture_scalar_operators() {
    assert_all("test/rust/crates/specgate-fixtures/specs/scalar_operators.spec.yaml");
}
#[test]
fn fixture_nested_structured() {
    assert_all("test/rust/crates/specgate-fixtures/specs/nested_structured.spec.yaml");
}
#[test]
fn fixture_structured_output() {
    assert_all("test/rust/crates/specgate-fixtures/specs/structured_output.spec.yaml");
}
#[test]
fn fixture_structured_map() {
    assert_all("test/rust/crates/specgate-fixtures/specs/structured_map.spec.yaml");
}
#[test]
fn fixture_structured_set() {
    assert_all("test/rust/crates/specgate-fixtures/specs/structured_set.spec.yaml");
}
#[test]
fn fixture_cross_dep() {
    assert_all("test/rust/crates/specgate-fixtures/specs/cross_dep.spec.yaml");
}
