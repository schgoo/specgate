//! Tests generated from `specs/specgate.cli.spec.yaml`.

use specgate_cli::run::{self, RunOutcome};
use specgate_cli::validate::{self, Severity, ValidateOutcome, ValidationFinding};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-cli
    p.pop(); // crates
    p.pop(); // rust
    p
}

fn vdir(rel: &str) -> String {
    repo_root().join(rel).to_string_lossy().to_string()
}

fn do_validate(rel: &str, strict: bool, suppress: &[&str]) -> ValidateOutcome {
    let sup: Vec<String> = suppress.iter().map(|s| s.to_string()).collect();
    validate::validate(&vdir(rel), strict, &sup)
}

fn pass_report(o: ValidateOutcome) -> validate::ValidationReport {
    match o {
        ValidateOutcome::Pass { report } => report,
        ValidateOutcome::Fail { report } => {
            panic!("expected Pass, got Fail; report={:#?}", report)
        }
    }
}

fn fail_report(o: ValidateOutcome) -> validate::ValidationReport {
    match o {
        ValidateOutcome::Fail { report } => report,
        ValidateOutcome::Pass { report } => {
            panic!("expected Fail, got Pass; report={:#?}", report)
        }
    }
}

fn has_finding(findings: &[ValidationFinding], sev: Severity, check: &str, message: &str) -> bool {
    findings
        .iter()
        .any(|f| f.severity == sev && f.check == check && f.message == message)
}

// ---------------------------------------------------------------------------
// Validate — 1. Schema
// ---------------------------------------------------------------------------

#[test]
fn validate_schema_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.errors, 0, "report={:#?}", r);
}

#[test]
fn validate_schema_missing_version() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/missing_version",
        false,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Error,
            "schema",
            "missing required field 'spec_version'"
        ),
        "missing schema finding: {:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 2. Operation reference
// ---------------------------------------------------------------------------

#[test]
fn validate_op_ref_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert!(r.findings.is_empty(), "expected no findings: {:#?}", r);
}

#[test]
fn validate_op_ref_fail() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/bad_op_ref",
        false,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Error,
            "operation_reference",
            "case 'test_case' references undefined operation 'nonexistent'"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 3. Name uniqueness
// ---------------------------------------------------------------------------

#[test]
fn validate_names_unique_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.errors, 0);
}

#[test]
fn validate_names_duplicate_fail() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/duplicate_names",
        false,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Error,
            "name_uniqueness",
            "duplicate case name 'my_case'"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 4. Provenance
// ---------------------------------------------------------------------------

#[test]
fn validate_provenance_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 0, "report={:#?}", r);
}

#[test]
fn validate_provenance_missing_warn() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/no_provenance",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Warn,
            "provenance",
            "case 'my_case' has no source.assertion_ids"
        ),
        "{:#?}",
        r.findings
    );
}

#[test]
fn validate_provenance_strict_fail() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/no_provenance",
        true,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
}

// ---------------------------------------------------------------------------
// Validate — 5. Input completeness
// ---------------------------------------------------------------------------

#[test]
fn validate_inputs_complete_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.errors, 0);
}

#[test]
fn validate_inputs_missing_fail() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/input_mismatch",
        false,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Error,
            "input_completeness",
            "case 'missing_input' missing required input 'b' for operation 'add'"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 6. Expected format
// ---------------------------------------------------------------------------

#[test]
fn validate_expected_format_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.errors, 0);
}

#[test]
fn validate_expected_format_fail() {
    let r = fail_report(do_validate(
        "test/fixtures/validation/bad_expected",
        false,
        &[],
    ));
    assert_eq!(r.errors, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Error,
            "expected_format",
            "case 'bad_format' has expected entry with multiple keys"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 7. Narrative misuse
// ---------------------------------------------------------------------------

#[test]
fn validate_narrative_proper_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 0);
}

#[test]
fn validate_narrative_misuse_warn() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/narrative_misuse",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Warn,
            "narrative_misuse",
            "narrative case 'should_be_runnable' has verify steps that look testable"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 8. Assertion coverage (narrative)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "narrative case: assertion_coverage requires --assertions-dir; not wired"]
fn validate_coverage_not_applicable() {}

// ---------------------------------------------------------------------------
// Validate — 9. Level match
// ---------------------------------------------------------------------------

#[test]
fn validate_level_match_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 0);
}

#[test]
fn validate_level_mismatch_warn() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/level_mismatch",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Warn,
            "level_match",
            "case 'required_but_marked_may' has level 'may' but source assertion TEST-MUST-1 appears to be MUST"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — 10. Negative coverage
// ---------------------------------------------------------------------------

#[test]
fn validate_negative_coverage_pass() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/valid_complete",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 0);
}

#[test]
fn validate_negative_coverage_warn() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/no_negative_case",
        false,
        &[],
    ));
    assert_eq!(r.warnings, 1, "report={:#?}", r);
    assert!(
        has_finding(
            &r.findings,
            Severity::Warn,
            "negative_coverage",
            "operation 'parse' has MUST cases but no error/rejection case"
        ),
        "{:#?}",
        r.findings
    );
}

// ---------------------------------------------------------------------------
// Validate — suppress
// ---------------------------------------------------------------------------

#[test]
fn validate_suppress_single_check() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/no_provenance",
        false,
        &["provenance"],
    ));
    assert_eq!(r.warnings, 0, "report={:#?}", r);
}

#[test]
fn validate_suppress_multiple_checks() {
    let r = pass_report(do_validate(
        "test/fixtures/validation/level_mismatch",
        false,
        &["provenance", "level_match"],
    ));
    assert_eq!(r.warnings, 0, "report={:#?}", r);
}

// ---------------------------------------------------------------------------
// Run — outcomes
// ---------------------------------------------------------------------------

fn do_run(rel: &str) -> RunOutcome {
    let p = repo_root().join(rel);
    run::run(p.to_str().unwrap())
}

fn complete(o: RunOutcome) -> run::RunReport {
    match o {
        RunOutcome::Complete { report } => report,
        RunOutcome::Error { reason } => panic!("expected Complete, got Error: {reason}"),
    }
}

fn err_reason(o: RunOutcome) -> String {
    match o {
        RunOutcome::Error { reason } => reason,
        RunOutcome::Complete { report } => panic!("expected Error, got Complete: {:#?}", report),
    }
}

#[test]
fn run_all_pass() {
    let r = complete(do_run(
        "test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml",
    ));
    assert_eq!(r.spec_name, "fixture.stateless_add");
    assert_eq!(r.total_cases, 1);
    assert_eq!(r.passed, 1);
    assert_eq!(r.failed, 0);
    assert_eq!(r.skipped, 0);
    assert_eq!(r.warned, 0);
}

#[test]
fn run_with_failure() {
    let r = complete(do_run(
        "test/rust/crates/specgate-fixtures/specs/statemachine_counter_wrong.spec.yaml",
    ));
    assert_eq!(r.spec_name, "fixture.statemachine_counter_wrong");
    assert_eq!(r.total_cases, 1);
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 1);
    assert_eq!(r.skipped, 0);
    assert_eq!(r.warned, 0);
}

#[test]
fn run_with_skip() {
    let r = complete(do_run(
        "test/rust/crates/specgate-fixtures/specs/level_may_missing.spec.yaml",
    ));
    assert_eq!(r.total_cases, 1);
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 0);
    assert_eq!(r.skipped, 1);
    assert_eq!(r.warned, 0);
}

#[test]
fn run_with_warn() {
    let r = complete(do_run(
        "test/rust/crates/specgate-fixtures/specs/level_should_missing.spec.yaml",
    ));
    assert_eq!(r.total_cases, 1);
    assert_eq!(r.passed, 0);
    assert_eq!(r.failed, 0);
    assert_eq!(r.skipped, 0);
    assert_eq!(r.warned, 1);
}

#[test]
fn run_nonexistent_spec() {
    let reason = err_reason(run::run("does/not/exist.spec.yaml"));
    assert_eq!(reason, "spec file not found: does/not/exist.spec.yaml");
}

#[test]
fn run_invalid_yaml() {
    let r = do_run("test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml");
    let reason = err_reason(r);
    assert_eq!(reason, "spec file is not valid YAML");
}

// ---------------------------------------------------------------------------
// Narrative cases
// ---------------------------------------------------------------------------

#[test]
#[ignore = "narrative: exit-code behavior verified by spawning the binary"]
fn cli_exit_codes() {}

#[test]
#[ignore = "narrative: terminal output is rendered by format_outcome helpers"]
fn terminal_output_format() {}
