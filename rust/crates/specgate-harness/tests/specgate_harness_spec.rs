//! Hand-written integration tests for `specs/specgate.harness.spec.yaml`.
//!
//! These call `run_spec()` directly and check the output matches the
//! harness spec's expected values. They provide fast feedback during
//! development. The harness also tests itself via `harness_self_test.rs`
//! which runs the full harness pipeline (codegen → compile → traces).

use specgate_harness::{run_spec, Assertion, CaseLevel, CaseResult, CaseStatus, RunOutcome, TraceEvent};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p
}

fn run(rel: &str) -> RunOutcome {
    let p = repo_root().join(rel);
    run_spec(p.to_str().unwrap())
}

fn complete(o: RunOutcome) -> Vec<CaseResult> {
    match o {
        RunOutcome::Complete { results } => results,
        RunOutcome::Error { reason } => panic!("expected Complete, got Error: {reason}"),
    }
}

fn err_reason(o: RunOutcome) -> String {
    match o {
        RunOutcome::Complete { .. } => panic!("expected Error, got Complete"),
        RunOutcome::Error { reason } => reason,
    }
}

fn ev(name: &str, value: &str) -> TraceEvent {
    TraceEvent::Event {
        name: name.into(),
        value: value.into(),
    }
}

fn run_op(op: &str) -> TraceEvent {
    TraceEvent::Run {
        operation: op.into(),
    }
}

fn aev(name: &str, value: &str) -> Assertion {
    Assertion::Event {
        name: name.into(),
        value: value.into(),
    }
}

fn arun(op: &str) -> Assertion {
    Assertion::Run {
        operation: op.into(),
    }
}

fn check_case(c: &CaseResult, name: &str, status: CaseStatus) {
    assert_eq!(c.name, name, "case name");
    assert_eq!(
        c.status, status,
        "case status for {name}, expected={:?}, traces={:?}",
        c.expected, c.traces
    );
}

// ---------------------------------------------------------------------------
// Happy path — basic operations
// ---------------------------------------------------------------------------

#[test]
fn stateless_return_value() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml",
    ));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "add_2_3", CaseStatus::Pass);
    assert_eq!(r[0].expected, vec![aev("add.result", "5")]);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("add"),
            ev("add.a", "2"),
            ev("add.b", "3"),
            ev("add.result", "5"),
        ]
    );
}

#[test]
fn statemachine_before_after() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/statemachine_counter.spec.yaml",
    ));
    check_case(&r[0], "increment_once", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![ev("count", "0"), run_op("increment"), ev("count", "1")]
    );
}

#[test]
fn multi_field_capture() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_field_capture.spec.yaml",
    ));
    check_case(&r[0], "withdraw_50", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("balance", "100"),
            ev("transaction_count", "0"),
            run_op("withdraw"),
            ev("balance", "50"),
            ev("transaction_count", "1"),
        ]
    );
}

#[test]
fn inline_checkpoint() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/checkpoint_inline.spec.yaml",
    ));
    check_case(&r[0], "process_hello", CaseStatus::Pass);
    // Macro echoes `&str` params, so `process.data` appears alongside the
    // inline `after_upper` checkpoint and the `process.result` event.
    assert_eq!(
        r[0].traces,
        vec![
            run_op("process"),
            ev("process.data", " hello "),
            ev("after_upper", " HELLO "),
            ev("process.result", "HELLO"),
        ]
    );
}

#[test]
fn multi_mutation() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_mutation.spec.yaml",
    ));
    check_case(&r[0], "double_increment", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("count", "0"),
            run_op("increment_twice"),
            ev("count", "1"),
            ev("count", "2"),
        ]
    );
}

#[test]
fn nested_operations() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/nested_operations.spec.yaml",
    ));
    check_case(&r[0], "transfer_50", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("balance", "100"),
            run_op("transfer"),
            run_op("withdraw"),
            ev("balance", "50"),
            run_op("deposit"),
            ev("balance", "100"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Setup variations
// ---------------------------------------------------------------------------

#[test]
fn setup_with_input_params() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/setup_with_params.spec.yaml",
    ));
    check_case(&r[0], "start_at_10", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("make_counter.initial", "10"),
            ev("count", "10"),
            run_op("increment"),
            ev("count", "11"),
        ]
    );
}

#[test]
fn multiple_setups() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_setup.spec.yaml",
    ));
    check_case(&r[0], "transfer_between_accounts", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("source.balance", "100"),
            ev("target.balance", "0"),
            run_op("transfer"),
            ev("source.balance", "50"),
            ev("target.balance", "50"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Multi-case / multi-step
// ---------------------------------------------------------------------------

#[test]
fn multiple_cases_one_spec() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_case.spec.yaml",
    ));
    assert_eq!(r.len(), 2);
    check_case(&r[0], "add_2_3", CaseStatus::Pass);
    check_case(&r[1], "add_10_20", CaseStatus::Pass);
}

#[test]
fn multi_step_sequence() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_step.spec.yaml",
    ));
    check_case(&r[0], "increment_then_decrement", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("count", "0"),
            run_op("increment"),
            ev("count", "1"),
            run_op("decrement"),
            ev("count", "0"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

#[test]
fn mock_call_site() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mock_field.spec.yaml",
    ));
    check_case(&r[0], "find_user_1", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("get_user"),
            ev("db.request", "user_1"),
            ev("db.response", "Alice"),
            ev("get_user.result", "Alice"),
        ]
    );
}

#[test]
fn mock_multi_response() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mock_multi_response.spec.yaml",
    ));
    check_case(&r[0], "get_two_different_users", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("get_users"),
            ev("db.request", "user_1"),
            ev("db.response", "Alice"),
            ev("db.request", "user_2"),
            ev("db.response", "Bob"),
            ev("get_users.result", "Alice and Bob"),
        ]
    );
}

#[test]
fn mock_input_not_in_table() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mock_not_found.spec.yaml",
    ));
    check_case(&r[0], "query_unknown_user", CaseStatus::Pass);
}

// ---------------------------------------------------------------------------
// Result and special returns
// ---------------------------------------------------------------------------

#[test]
fn result_ok_path() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/result_ok.spec.yaml",
    ));
    check_case(&r[0], "divide_10_by_2", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![ev("divide.outcome", "Ok"), ev("divide.result", "5")]
    );
}

#[test]
fn result_err_path() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/result_err.spec.yaml",
    ));
    check_case(&r[0], "divide_by_zero", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("divide.outcome", "Error"),
            ev("divide.error", "division by zero"),
        ]
    );
}

#[test]
fn unrecoverable_panic() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/unrecoverable.spec.yaml",
    ));
    check_case(&r[0], "divide_by_zero_panics", CaseStatus::Pass);
}

#[test]
fn void_operation() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/void_operation.spec.yaml",
    ));
    check_case(&r[0], "log_a_message", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![ev("count", "0"), run_op("log"), ev("count", "1")]
    );
}

#[test]
fn readonly_operation() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/readonly_operation.spec.yaml",
    ));
    check_case(&r[0], "read_count", CaseStatus::Pass);
}

// ---------------------------------------------------------------------------
// Subsequence behavior
// ---------------------------------------------------------------------------

#[test]
fn event_order_between_runs() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/multi_field_capture_reordered.spec.yaml",
    ));
    check_case(&r[0], "withdraw_50", CaseStatus::Pass);
}

#[test]
fn subsequence_with_gaps() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/subsequence_with_gaps.spec.yaml",
    ));
    check_case(&r[0], "double_increment", CaseStatus::Pass);
}

#[test]
fn subsequence_wrong_order() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/subsequence_wrong_order.spec.yaml",
    ));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

// ---------------------------------------------------------------------------
// Mismatches
// ---------------------------------------------------------------------------

#[test]
fn mismatch_wrong_value() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/statemachine_counter_wrong.spec.yaml",
    ));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

#[test]
fn mismatch_missing_field() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mismatch_missing_event.spec.yaml",
    ));
    check_case(&r[0], "add_2_3", CaseStatus::Fail);
}

#[test]
fn mismatch_wrong_field_name() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mismatch_wrong_field.spec.yaml",
    ));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

#[test]
fn mismatch_second_step() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/mismatch_second_step.spec.yaml",
    ));
    check_case(&r[0], "increment_then_decrement", CaseStatus::Fail);
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn error_bad_yaml() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml",
    ));
    assert_eq!(reason, "spec file is not valid YAML");
}

#[test]
fn error_bad_binding() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/bad_binding.spec.yaml",
    ));
    assert_eq!(reason, "binding 'nonexistent' not found");
}

#[test]
fn error_missing_setup() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/missing_setup.spec.yaml",
    ));
    assert_eq!(reason, "setup 'make_counter' not found in source annotations");
}

#[test]
fn error_missing_operation() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/missing_operation.spec.yaml",
    ));
    assert_eq!(reason, "operation 'increment' not found in source annotations");
}

#[test]
fn error_compile_failure() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/compile_error.spec.yaml",
    ));
    assert_eq!(reason, "source failed to compile");
}

#[test]
fn error_no_cases() {
    let reason = err_reason(run(
        "test/rust/crates/specgate-fixtures/specs/no_cases.spec.yaml",
    ));
    assert_eq!(reason, "spec has no test cases");
}

// ---------------------------------------------------------------------------
// v0.4.0: $run / $unordered / $anywhere directives
// ---------------------------------------------------------------------------

#[test]
fn async_operation() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/async_fetch.spec.yaml",
    ));
    check_case(&r[0], "fetch_returns_response", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("fetch"),
            ev("fetch.url", "https://example.com"),
            ev("fetch.result", "response from https://example.com"),
        ]
    );
}

#[test]
fn keyword_collision_operation_named_run() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/keyword_collision.spec.yaml",
    ));
    check_case(&r[0], "operation_named_run", CaseStatus::Pass);
    assert_eq!(
        r[0].expected,
        vec![arun("run"), aev("run.result", "executed: test")]
    );
}

#[test]
fn unordered_field_matching() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/unordered_fields.spec.yaml",
    ));
    assert_eq!(r.len(), 4);
    check_case(&r[0], "unordered_both_present", CaseStatus::Pass);
    check_case(&r[1], "unordered_reversed_still_passes", CaseStatus::Pass);
    check_case(&r[2], "unordered_wrong_value_fails", CaseStatus::Fail);
    check_case(&r[3], "unordered_multiple_blocks", CaseStatus::Pass);
}

#[test]
fn anywhere_event_matching() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/anywhere_event.spec.yaml",
    ));
    assert_eq!(r.len(), 5);
    check_case(&r[0], "anywhere_matches_early_event", CaseStatus::Pass);
    check_case(&r[1], "anywhere_matches_late_event", CaseStatus::Pass);
    check_case(&r[2], "anywhere_matches_middle_event", CaseStatus::Pass);
    check_case(&r[3], "anywhere_multiple_items", CaseStatus::Pass);
    check_case(&r[4], "anywhere_missing_fails", CaseStatus::Fail);
}

// ---------------------------------------------------------------------------
// v0.4.0: level + source provenance
// ---------------------------------------------------------------------------

#[test]
fn provenance_passes_through() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/provenance_example.spec.yaml",
    ));
    check_case(&r[0], "add_with_provenance", CaseStatus::Pass);
    assert_eq!(r[0].level, CaseLevel::Must);
    let src = r[0].source.as_ref().expect("source missing");
    assert_eq!(src.assertion_ids, vec!["TEST-A1", "TEST-A2"]);
    assert_eq!(src.spec, "Test Specification v1.0");
    assert_eq!(src.section, "§3.1");
}

#[test]
fn level_may_missing_skips() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/level_may_missing.spec.yaml",
    ));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "optional_not_implemented", CaseStatus::Skip);
    assert_eq!(r[0].level, CaseLevel::May);
    assert!(r[0].expected.is_empty());
    assert!(r[0].traces.is_empty());
}

#[test]
fn level_should_missing_warns() {
    let r = complete(run(
        "test/rust/crates/specgate-fixtures/specs/level_should_missing.spec.yaml",
    ));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "recommended_not_implemented", CaseStatus::Warn);
    assert_eq!(r[0].level, CaseLevel::Should);
    assert!(r[0].expected.is_empty());
    assert!(r[0].traces.is_empty());
}
