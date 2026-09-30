//! Thin CLI formatting for native CTSC validation.

use specgate_ctsc::validation::ValidationReport;
use std::fmt::Write as _;

/// Format a CTSC validation report for command-line output.
///
/// # Examples
///
/// Construct a [`ValidationReport`] and pass it to this function to obtain the
/// same stable text emitted by the `validate` command.
#[must_use]
pub fn format_report(report: &ValidationReport) -> String {
    let mut output = String::new();
    if report.valid {
        let _ = writeln!(output, "valid: {} {}", report.level, report.artifact);
    } else {
        let _ = writeln!(output, "invalid: {} {}", report.level, report.artifact);
        for issue in &report.issues {
            let _ = writeln!(output, "  {}: {}", issue.location, issue.message);
        }
    }
    output
}
