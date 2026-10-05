//! Private CLI rendering for CTSC reports.

use specgate_ctsc::comparison::ComparisonReport;
use std::fmt::Write as _;

// Capacity hints cover the fixed heading lines and a typical rendered detail
// line. They affect allocation only and never the command output contract.
const COMPARISON_CAPACITY: usize = 160;
const VALIDATION_CAPACITY: usize = 80;
const DETAIL_CAPACITY: usize = 80;

/// Format a CTSC Strict report for command-line output.
///
/// # Examples
///
/// ```
/// # use crate::reporting::format_comparison;
/// use specgate_ctsc::comparison::ComparisonReport;
/// let report = ComparisonReport {
///     policy: "ctsc.strict".into(), policy_version: "0.1.0".into(),
///     reference: "reference.json".into(), candidate: "candidate.json".into(),
///     equivalent: true, validation_failures: vec![], errors: vec![], mismatches: vec![],
/// };
/// assert!(format_comparison(&report).contains("equivalent: true"));
/// ```
#[must_use]
pub(super) fn format_comparison(report: &ComparisonReport) -> String {
    let mut output = String::with_capacity(
        COMPARISON_CAPACITY + DETAIL_CAPACITY * (report.validation_failures.len() + report.errors.len() + report.mismatches.len()),
    );
    let _ = writeln!(output, "policy: {}/{}", report.policy, report.policy_version);
    let _ = writeln!(output, "reference: {}", report.reference.display());
    let _ = writeln!(output, "candidate: {}", report.candidate.display());
    let _ = writeln!(output, "equivalent: {}", report.equivalent);
    for failure in &report.validation_failures {
        let _ = writeln!(output, "validation {}: {}", failure.location, failure.message);
    }
    for error in &report.errors {
        let _ = writeln!(output, "error {}: {}", error.path, error.message);
    }
    for mismatch in &report.mismatches {
        let _ = writeln!(
            output,
            "mismatch {}: expected {}, actual {}",
            mismatch.path, mismatch.expected, mismatch.actual
        );
    }
    output
}

use specgate_ctsc::validation::ValidationReport;

/// Format a CTSC validation report for command-line output.
///
/// # Examples
///
/// ```
/// # use crate::reporting::format_validation;
/// use specgate_ctsc::validation::ValidationReport;
/// let report = ValidationReport {
///     level: "trace".into(), artifact: "trace.json".into(), valid: true, issues: vec![],
/// };
/// assert_eq!(format_validation(&report), "valid: trace trace.json\n");
/// ```
#[must_use]
pub(super) fn format_validation(report: &ValidationReport) -> String {
    let mut output = String::with_capacity(VALIDATION_CAPACITY + DETAIL_CAPACITY * report.issues.len());
    if report.valid {
        let _ = writeln!(output, "valid: {} {}", report.level, report.artifact.display());
    } else {
        let _ = writeln!(output, "invalid: {} {}", report.level, report.artifact.display());
        for issue in &report.issues {
            let _ = writeln!(output, "  {}: {}", issue.location, issue.message);
        }
    }
    output
}

/// Render a capture result using the stable CLI line protocol.
///
/// ```
/// # use crate::reporting::format_capture;
/// use specgate_cli::{CapturePaths, CaptureRequest};
/// let outcome = CaptureRequest::builder(CapturePaths { binding: "".into(), out: "out".into() }).build();
/// assert!(format_capture(&outcome).starts_with("Error("));
/// ```
pub(super) fn format_capture(outcome: &Result<specgate_cli::CaptureReport, specgate_cli::CaptureError>) -> String {
    match outcome {
        Ok(report) => format!(
            "Complete(component={}, scenarios={}, operations={}, registry={}, trace={}, manifest={})\n",
            report.component_id(),
            report.scenarios(),
            report.operations(),
            report.registry_path.display(),
            report.trace_path.display(),
            report.manifest_path.display()
        ),
        Err(error) => format!("Error({})\n", error.diagnostic()),
    }
}

/// Render a replay result using the stable CLI line protocol.
///
/// ```
/// # use crate::reporting::format_replay;
/// use specgate_cli::replay::{Paths, Request};
/// let outcome = Request::builder(Paths::new("", "binding.yaml", "candidate.json")).build();
/// let rendered = format_replay(&outcome);
/// assert!(rendered.starts_with("Error("));
/// assert!(rendered.ends_with("\n"));
/// ```
pub(super) fn format_replay(outcome: &Result<specgate_cli::replay::Report, specgate_cli::replay::Error>) -> String {
    match outcome {
        Ok(report) => format!(
            "Complete(component={}, scenarios={}, operations={}, plans={}, output={})\n",
            report.component_id(),
            report.scenarios,
            report.operations,
            report.plans,
            report.output_path.display()
        ),
        Err(error) => format!("Error({})\n", error.diagnostic()),
    }
}
