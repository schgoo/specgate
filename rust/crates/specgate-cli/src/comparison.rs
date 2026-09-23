//! Thin CLI formatting for CTSC Strict comparison.

use specgate_ctsc::comparison::ComparisonReport;
use std::fmt::Write as _;

#[must_use]
pub fn format_report(report: &ComparisonReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "policy: {}/{}", report.policy, report.policy_version);
    let _ = writeln!(output, "reference: {}", report.reference);
    let _ = writeln!(output, "candidate: {}", report.candidate);
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
