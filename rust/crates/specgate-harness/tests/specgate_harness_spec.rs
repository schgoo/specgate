//! Hand-written integration tests for `specs/specgate.harness.spec.yaml`.
//!
//! These call `run_spec()` directly and check the output matches the
//! harness spec's expected values. They provide fast feedback during
//! development. The harness also tests itself via `harness_self_test.rs`
//! which runs the full harness pipeline (codegen → compile → traces).

use specgate_harness::{Assertion, CaseLevel, CaseResult, CaseStatus, RunOutcome, TraceEvent, run_spec};

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

fn ev<V: Into<specgate_harness::Value>>(name: &str, value: V) -> TraceEvent {
    TraceEvent::Event {
        name: name.into(),
        value: value.into(),
    }
}

fn run_op(op: &str) -> TraceEvent {
    TraceEvent::Run { operation: op.into() }
}

fn ev_map(name: &str, entries: Vec<(&str, specgate_harness::Value)>) -> TraceEvent {
    let map = entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    TraceEvent::Event {
        name: name.into(),
        value: specgate_harness::Value::Map(map),
    }
}

fn vmap(entries: Vec<(&str, specgate_harness::Value)>) -> specgate_harness::Value {
    specgate_harness::Value::Map(entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn aev<V: Into<specgate_harness::AssertValue>>(name: &str, value: V) -> Assertion {
    Assertion::Event {
        name: name.into(),
        value: value.into(),
    }
}

fn arun(op: &str) -> Assertion {
    Assertion::Run { operation: op.into() }
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
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "add_2_3", CaseStatus::Pass);
    assert_eq!(r[0].expected, vec![aev("$result", "5")]);
    assert_eq!(
        r[0].traces,
        vec![run_op("add"), ev("add.a", "2"), ev("add.b", "3"), ev("$result", "5"),]
    );
}

#[test]
fn statemachine_before_after() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/statemachine_counter.spec.yaml"));
    check_case(&r[0], "increment_once", CaseStatus::Pass);
    assert_eq!(r[0].traces, vec![ev("count", 0i64), run_op("increment"), ev("count", 1i64)]);
}

#[test]
fn multi_field_capture() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/multi_field_capture.spec.yaml"));
    check_case(&r[0], "withdraw_50", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("balance", 100i64),
            ev("transaction_count", 0i64),
            run_op("withdraw"),
            ev("balance", 50i64),
            ev("transaction_count", 1i64),
        ]
    );
}

#[test]
fn inline_checkpoint() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/checkpoint_inline.spec.yaml"));
    check_case(&r[0], "process_hello", CaseStatus::Pass);
    // Macro echoes `&str` params, so `process.data` appears alongside the
    // inline `after_upper` checkpoint and the `process.result` event.
    assert_eq!(
        r[0].traces,
        vec![
            run_op("process"),
            ev("process.data", " hello "),
            ev("after_upper", " HELLO "),
            ev("$result", "HELLO"),
        ]
    );
}

#[test]
fn multi_mutation() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/multi_mutation.spec.yaml"));
    check_case(&r[0], "double_increment", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![ev("count", 0i64), run_op("increment_twice"), ev("count", 1i64), ev("count", 2i64),]
    );
}

#[test]
fn nested_operations() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/nested_operations.spec.yaml"));
    check_case(&r[0], "transfer_50", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("balance", 100i64),
            run_op("transfer"),
            run_op("withdraw"),
            ev("balance", 50i64),
            run_op("deposit"),
            ev("balance", 100i64),
        ]
    );
}

// ---------------------------------------------------------------------------
// Setup variations
// ---------------------------------------------------------------------------

#[test]
fn setup_with_input_params() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/setup_with_params.spec.yaml"));
    check_case(&r[0], "start_at_10", CaseStatus::Pass);
    assert_eq!(r[0].traces, vec![ev("count", 10i64), run_op("increment"), ev("count", 11i64),]);
}

#[test]
fn multiple_setups() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/multi_setup.spec.yaml"));
    check_case(&r[0], "transfer_between_accounts", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("source.balance", 100i64),
            ev("target.balance", 0i64),
            run_op("transfer"),
            ev("transfer.amount", "50"),
            ev("source.balance", 50i64),
            ev("target.balance", 50i64),
        ]
    );
}

// ---------------------------------------------------------------------------
// Multi-case / multi-step
// ---------------------------------------------------------------------------

#[test]
fn multiple_cases_one_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/multi_case.spec.yaml"));
    assert_eq!(r.len(), 2);
    check_case(&r[0], "add_2_3", CaseStatus::Pass);
    check_case(&r[1], "add_10_20", CaseStatus::Pass);
}

#[test]
fn multi_step_sequence() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/multi_step.spec.yaml"));
    check_case(&r[0], "increment_then_decrement", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            ev("count", 0i64),
            run_op("increment"),
            ev("count", 1i64),
            run_op("decrement"),
            ev("count", 0i64),
        ]
    );
}

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

#[test]
fn mock_call_site() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mock_field.spec.yaml"));
    check_case(&r[0], "find_user_1", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("get_user"),
            ev("db.request", "user_1"),
            ev("db.response", "Alice"),
            ev("$result", "Alice"),
        ]
    );
}

#[test]
fn mock_multi_response() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mock_multi_response.spec.yaml"));
    check_case(&r[0], "get_two_different_users", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("get_users"),
            ev("db.request", "user_1"),
            ev("db.response", "Alice"),
            ev("db.request", "user_2"),
            ev("db.response", "Bob"),
            ev("$result", "Alice and Bob"),
        ]
    );
}

#[test]
fn mock_input_not_in_table() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mock_not_found.spec.yaml"));
    check_case(&r[0], "query_unknown_user", CaseStatus::Pass);
}

// ---------------------------------------------------------------------------
// Result and special returns
// ---------------------------------------------------------------------------

#[test]
fn result_ok_path() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/result_ok.spec.yaml"));
    check_case(&r[0], "divide_10_by_2", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("divide"),
            ev("divide.a", "10"),
            ev("divide.b", "2"),
            ev_map("$result", vec![("Ok", specgate_harness::Value::Integer(5))]),
        ]
    );
}

#[test]
fn result_err_path() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/result_err.spec.yaml"));
    check_case(&r[0], "divide_by_zero", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("divide"),
            ev("divide.a", "10"),
            ev("divide.b", "0"),
            ev_map("$result", vec![("Err", specgate_harness::Value::String("division by zero".into()))]),
        ]
    );
}

#[test]
fn unrecoverable_panic() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/unrecoverable.spec.yaml"));
    check_case(&r[0], "divide_by_zero_panics", CaseStatus::Pass);
}

#[test]
fn void_operation() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/void_operation.spec.yaml"));
    check_case(&r[0], "log_a_message", CaseStatus::Pass);
    assert_eq!(r[0].traces, vec![ev("count", 0i64), run_op("log"), ev("count", 1i64)]);
}

#[test]
fn readonly_operation() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/readonly_operation.spec.yaml"));
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
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/subsequence_with_gaps.spec.yaml"));
    check_case(&r[0], "double_increment", CaseStatus::Pass);
}

#[test]
fn subsequence_wrong_order() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/subsequence_wrong_order.spec.yaml"));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

// ---------------------------------------------------------------------------
// Mismatches
// ---------------------------------------------------------------------------

#[test]
fn mismatch_wrong_value() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/statemachine_counter_wrong.spec.yaml"));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

#[test]
fn mismatch_missing_field() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mismatch_missing_event.spec.yaml"));
    check_case(&r[0], "add_2_3", CaseStatus::Fail);
}

#[test]
fn mismatch_wrong_field_name() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mismatch_wrong_field.spec.yaml"));
    check_case(&r[0], "increment_once", CaseStatus::Fail);
}

#[test]
fn mismatch_second_step() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/mismatch_second_step.spec.yaml"));
    check_case(&r[0], "increment_then_decrement", CaseStatus::Fail);
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn error_bad_yaml() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml"));
    assert_eq!(reason, "spec file is not valid YAML");
}

#[test]
fn error_bad_binding() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/bad_binding.spec.yaml"));
    assert_eq!(reason, "binding 'nonexistent' not found");
}

#[test]
fn error_missing_setup() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/missing_setup.spec.yaml"));
    assert_eq!(
        reason,
        "case 'increment_once': operation 'increment' is a method on 'Counter' but no #[spec_setup(\"increment\")] returns 'Counter' to construct the receiver"
    );
}

#[test]
fn error_missing_operation() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/missing_operation.spec.yaml"));
    assert_eq!(reason, "operation 'increment' not found in source annotations");
}

#[test]
fn error_compile_failure() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/compile_error.spec.yaml"));
    assert_eq!(reason, "source failed to compile");
}

#[test]
fn error_no_cases() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/no_cases.spec.yaml"));
    assert_eq!(reason, "spec has no test cases");
}

// ---------------------------------------------------------------------------
// v0.4.0: $run / $unordered / $anywhere directives
// ---------------------------------------------------------------------------

#[test]
fn async_operation() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/async_fetch.spec.yaml"));
    check_case(&r[0], "fetch_returns_response", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![
            run_op("fetch"),
            ev("fetch.url", "https://example.com"),
            ev("$result", "response from https://example.com"),
        ]
    );
}

#[test]
fn keyword_collision_operation_named_run() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/keyword_collision.spec.yaml"));
    check_case(&r[0], "operation_named_run", CaseStatus::Pass);
    assert_eq!(r[0].expected, vec![arun("run"), aev("$result", "executed: test")]);
}

#[test]
fn unordered_field_matching() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/unordered_fields.spec.yaml"));
    assert_eq!(r.len(), 4);
    check_case(&r[0], "unordered_both_present", CaseStatus::Pass);
    check_case(&r[1], "unordered_reversed_still_passes", CaseStatus::Pass);
    check_case(&r[2], "unordered_wrong_value_fails", CaseStatus::Fail);
    check_case(&r[3], "unordered_multiple_blocks", CaseStatus::Pass);
}

#[test]
fn anywhere_event_matching() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/anywhere_event.spec.yaml"));
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
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/provenance_example.spec.yaml"));
    check_case(&r[0], "add_with_provenance", CaseStatus::Pass);
    assert_eq!(r[0].level, CaseLevel::Must);
    let src = r[0].source.as_ref().expect("source missing");
    assert_eq!(src.assertion_ids, vec!["TEST-A1", "TEST-A2"]);
    assert_eq!(src.spec, "Test Specification v1.0");
    assert_eq!(src.section, "§3.1");
}

#[test]
fn level_may_missing_skips() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/level_may_missing.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "optional_not_implemented", CaseStatus::Skip);
    assert_eq!(r[0].level, CaseLevel::May);
    assert!(r[0].expected.is_empty());
    assert!(r[0].traces.is_empty());
}

#[test]
fn level_should_missing_warns() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/level_should_missing.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "recommended_not_implemented", CaseStatus::Warn);
    assert_eq!(r[0].level, CaseLevel::Should);
    assert!(r[0].expected.is_empty());
    assert!(r[0].traces.is_empty());
}

// ---------------------------------------------------------------------------
// Vacuous matching — non-empty expected with zero matches must fail
// ---------------------------------------------------------------------------

#[test]
fn no_vacuous_match() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/vacuous_match.spec.yaml"));
    assert_eq!(r.len(), 2);
    // Wrong value — must fail, not pass
    check_case(&r[0], "wrong_value_fails", CaseStatus::Fail);
    assert!(!r[0].traces.is_empty(), "operation ran, traces should exist");
    // $run for non-matching operation — must fail, not vacuously pass
    check_case(&r[1], "wrong_run_fails", CaseStatus::Fail);
    assert!(!r[1].traces.is_empty(), "operation ran, traces should exist");
}

// ---------------------------------------------------------------------------
// Path absolutization — specs at nested paths resolve correctly
// ---------------------------------------------------------------------------

#[test]
fn paths_resolve_from_nested_spec() {
    let r = complete(run("test/fixtures/nested/deep/path/nested_path.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "add_from_nested_path", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![run_op("add"), ev("add.a", "10"), ev("add.b", "20"), ev("$result", "30"),]
    );
}

// ---------------------------------------------------------------------------
// Target selection — multi-target binding
// ---------------------------------------------------------------------------

#[test]
fn target_selection() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/target_selection.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "greet_world", CaseStatus::Pass);
    assert_eq!(
        r[0].traces,
        vec![run_op("greet"), ev("greet.name", "World"), ev("$result", "Hello, World!"),]
    );
}

#[test]
fn per_case_target_override() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/per_case_target.spec.yaml"));
    assert_eq!(r.len(), 2);
    // First case uses default target (add from specgate-fixtures)
    check_case(&r[0], "add_from_default", CaseStatus::Pass);
    assert!(
        r[0].traces
            .iter()
            .any(|t| matches!(t, TraceEvent::Run { operation, .. } if operation == "add"))
    );
    // Second case uses alt target (greet from specgate-fixtures-alt)
    check_case(&r[1], "greet_from_alt", CaseStatus::Pass);
    assert!(
        r[1].traces
            .iter()
            .any(|t| matches!(t, TraceEvent::Run { operation, .. } if operation == "greet"))
    );
}

#[test]
fn missing_target_error() {
    let reason = err_reason(run("test/rust/crates/specgate-fixtures/specs/missing_target.spec.yaml"));
    assert_eq!(reason, "target 'nonexistent' not found in binding");
}

// ---------------------------------------------------------------------------
// Structured value + operator specs
// ---------------------------------------------------------------------------

#[test]
fn structured_output_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/structured_output.spec.yaml"));
    assert_eq!(r.len(), 5);
    check_case(&r[0], "list_exact_match", CaseStatus::Pass);
    check_case(&r[1], "list_contains_check", CaseStatus::Pass);
    check_case(&r[2], "list_size_check", CaseStatus::Pass);
    check_case(&r[3], "list_any_operator", CaseStatus::Pass);
    check_case(&r[4], "list_wrong_value_fails", CaseStatus::Fail);
}

#[test]
fn structured_map_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/structured_map.spec.yaml"));
    assert_eq!(r.len(), 4);
    check_case(&r[0], "map_key_value_match", CaseStatus::Pass);
    check_case(&r[1], "map_subset_match", CaseStatus::Pass);
    check_case(&r[2], "map_wrong_value_fails", CaseStatus::Fail);
    check_case(&r[3], "map_size_check", CaseStatus::Pass);
}

#[test]
fn structured_set_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/structured_set.spec.yaml"));
    assert_eq!(r.len(), 5);
    check_case(&r[0], "set_presence_match", CaseStatus::Pass);
    check_case(&r[1], "set_all_items", CaseStatus::Pass);
    check_case(&r[2], "set_missing_item_fails", CaseStatus::Fail);
    check_case(&r[3], "set_size_check", CaseStatus::Pass);
    check_case(&r[4], "set_contains_check", CaseStatus::Pass);
}

#[test]
fn operators_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/operators.spec.yaml"));
    assert_eq!(r.len(), 12);
    check_case(&r[0], "eq_scalar", CaseStatus::Pass);
    check_case(&r[1], "size_list", CaseStatus::Pass);
    check_case(&r[2], "size_map", CaseStatus::Pass);
    check_case(&r[3], "contains_single", CaseStatus::Pass);
    check_case(&r[4], "contains_all", CaseStatus::Pass);
    check_case(&r[5], "excludes_values", CaseStatus::Pass);
    check_case(&r[6], "excludes_fails_when_present", CaseStatus::Fail);
    check_case(&r[7], "match_partial_object", CaseStatus::Pass);
    check_case(&r[8], "exists_field", CaseStatus::Pass);
    check_case(&r[9], "any_with_matcher", CaseStatus::Pass);
    check_case(&r[10], "type_check", CaseStatus::Pass);
    check_case(&r[11], "composed_operators", CaseStatus::Pass);
}

#[test]
fn scalar_operators_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/scalar_operators.spec.yaml"));
    assert_eq!(r.len(), 13);
    check_case(&r[0], "regex_match", CaseStatus::Pass);
    check_case(&r[1], "regex_no_match_fails", CaseStatus::Fail);
    check_case(&r[2], "not_value", CaseStatus::Pass);
    check_case(&r[3], "not_fails_when_equal", CaseStatus::Fail);
    check_case(&r[4], "gt_passes", CaseStatus::Pass);
    check_case(&r[5], "gte_passes_equal", CaseStatus::Pass);
    check_case(&r[6], "lt_passes", CaseStatus::Pass);
    check_case(&r[7], "lte_fails", CaseStatus::Fail);
    check_case(&r[8], "empty_list", CaseStatus::Pass);
    check_case(&r[9], "every_element", CaseStatus::Pass);
    check_case(&r[10], "every_fails", CaseStatus::Fail);
    check_case(&r[11], "combined_multi_field", CaseStatus::Pass);
    check_case(&r[12], "combined_multi_field_one_fails", CaseStatus::Fail);
}

#[test]
fn nested_structured_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/nested_structured.spec.yaml"));
    assert_eq!(r.len(), 4);
    check_case(&r[0], "nested_list_of_maps_exact", CaseStatus::Pass);
    check_case(&r[1], "nested_any_with_match", CaseStatus::Pass);
    check_case(&r[2], "nested_size", CaseStatus::Pass);
    check_case(&r[3], "nested_contains_element", CaseStatus::Pass);
}

#[test]
fn cross_dep_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/cross_dep.spec.yaml"));
    assert_eq!(r.len(), 1);
    check_case(&r[0], "extract_yaml_value", CaseStatus::Pass);
}

// ---------------------------------------------------------------------------
// Enum SpecEvent derive
// ---------------------------------------------------------------------------

#[test]
fn enum_event_spec() {
    let r = complete(run("test/rust/crates/specgate-fixtures/specs/enum_event.spec.yaml"));
    assert_eq!(r.len(), 3);
    check_case(&r[0], "unit_variant", CaseStatus::Pass);
    check_case(&r[1], "single_field_variant", CaseStatus::Pass);
    check_case(&r[2], "multi_field_variant", CaseStatus::Pass);
    // Spot-check that the structured `$result` enum events appear in traces.
    assert!(r[0].traces.contains(&ev_map("$result", vec![("Point", vmap(vec![]))])));
    assert!(r[1].traces.contains(&ev_map(
        "$result",
        vec![("Circle", vmap(vec![("radius", specgate_harness::Value::Float(5.0))]))]
    )));
    assert!(r[2].traces.contains(&ev_map(
        "$result",
        vec![(
            "Rectangle",
            vmap(vec![
                ("width", specgate_harness::Value::Float(3.0)),
                ("height", specgate_harness::Value::Float(4.0)),
            ])
        )]
    )));
}
