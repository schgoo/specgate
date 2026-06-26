//! `specgate run <spec>` — wrap the harness and produce a [`RunReport`].

use std::fmt::Write as _;
use std::path::Path;

use specgate::{SpecEvent, spec_operation};
use specgate_harness::{CaseStatus, CoverageOutcome, RunOutcome as HarnessOutcome};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct RunReport {
    #[spec_event]
    pub spec_name: String,
    #[spec_event]
    pub total_cases: i32,
    #[spec_event]
    pub passed: i32,
    #[spec_event]
    pub failed: i32,
    #[spec_event]
    pub skipped: i32,
    #[spec_event]
    pub warned: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum RunOutcome {
    Complete { report: RunReport },
    Error { reason: String },
}

impl std::fmt::Display for RunOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunOutcome::Complete { report } => write!(
                f,
                "Complete(spec={}, total={}, passed={}, failed={}, skipped={}, warned={})",
                report.spec_name, report.total_cases, report.passed, report.failed, report.skipped, report.warned
            ),
            RunOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

#[spec_operation("run")]
pub fn run(spec: &str) -> RunOutcome {
    let spec_path = spec;
    if !Path::new(spec_path).exists() {
        return RunOutcome::Error {
            reason: format!("spec file not found: {spec_path}"),
        };
    }

    let spec_name = read_spec_name(spec_path).unwrap_or_default();

    match specgate_harness::run_spec(spec_path) {
        HarnessOutcome::Error { reason } => RunOutcome::Error { reason },
        HarnessOutcome::Complete { results } => {
            let mut report = RunReport {
                spec_name,
                total_cases: i32::try_from(results.len()).expect("case count fits i32"),
                passed: 0,
                failed: 0,
                skipped: 0,
                warned: 0,
            };
            for r in &results {
                match r.status {
                    CaseStatus::Pass => report.passed += 1,
                    CaseStatus::Fail => report.failed += 1,
                    CaseStatus::Skip => report.skipped += 1,
                    CaseStatus::Warn => report.warned += 1,
                }
            }
            RunOutcome::Complete { report }
        }
    }
}

fn read_spec_name(spec_path: &str) -> Option<String> {
    let text = std::fs::read_to_string(spec_path).ok()?;
    let v: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;
    v.get("name")?.as_str().map(ToString::to_string)
}

/// Run a spec with code-coverage measurement of the implementation crate(s)
/// under test. Returns the harness's [`CoverageOutcome`].
#[must_use]
pub fn run_with_coverage(spec: &str) -> CoverageOutcome {
    if !Path::new(spec).exists() {
        return CoverageOutcome::Error {
            reason: format!("spec file not found: {spec}"),
        };
    }
    specgate_harness::run_spec_with_coverage(spec)
}

/// Process exit code for a coverage run. Non-zero when the spec errored, any
/// case failed, or (when a `threshold` is given and coverage was measured) the
/// covered-line percentage is below the threshold. A run whose coverage could
/// not be measured (`Unavailable`) does not fail on threshold — coverage simply
/// wasn't enforced.
#[must_use]
pub fn coverage_exit_code(outcome: &CoverageOutcome, threshold: Option<f64>) -> u8 {
    match outcome {
        CoverageOutcome::Error { .. } => 1,
        CoverageOutcome::Unavailable { results, .. } => u8::from(any_failed(results)),
        CoverageOutcome::Measured { results, coverage } => {
            if any_failed(results) {
                return 1;
            }
            match threshold {
                Some(t) if coverage.percent < t => 1,
                _ => 0,
            }
        }
    }
}

fn any_failed(results: &[specgate_harness::CaseResult]) -> bool {
    results.iter().any(|r| r.status == CaseStatus::Fail)
}

/// Render a coverage outcome to a colored, human-readable string.
#[must_use]
pub fn format_coverage(outcome: &CoverageOutcome) -> String {
    let mut s = String::new();
    match outcome {
        CoverageOutcome::Error { reason } => {
            writeln!(s, "\x1b[31merror:\x1b[0m {reason}").unwrap();
        }
        CoverageOutcome::Unavailable { results, reason } => {
            write_case_summary(&mut s, results);
            writeln!(s, "\x1b[33mcoverage unavailable:\x1b[0m {reason}").unwrap();
        }
        CoverageOutcome::Measured { results, coverage } => {
            write_case_summary(&mut s, results);
            writeln!(
                s,
                "\x1b[36mcoverage:\x1b[0m {}/{} lines ({:.1}%) across {} file(s)",
                coverage.lines_covered,
                coverage.lines_total,
                coverage.percent,
                coverage.files.len()
            )
            .unwrap();
            for f in &coverage.files {
                writeln!(s, "  {} — {:.1}%", f.path, f.percent).unwrap();
            }
        }
    }
    s
}

fn write_case_summary(s: &mut String, results: &[specgate_harness::CaseResult]) {
    let (mut passed, mut failed, mut skipped, mut warned) = (0, 0, 0, 0);
    for r in results {
        match r.status {
            CaseStatus::Pass => passed += 1,
            CaseStatus::Fail => failed += 1,
            CaseStatus::Skip => skipped += 1,
            CaseStatus::Warn => warned += 1,
        }
    }
    writeln!(
        s,
        "\x1b[32mpassed:\x1b[0m {passed} \x1b[31mfailed:\x1b[0m {failed} \x1b[33mwarned:\x1b[0m {warned} \x1b[36mskipped:\x1b[0m {skipped} (total {})",
        results.len()
    )
    .unwrap();
}

/// Render a run outcome to a colored, human-readable string for the
/// terminal (used by the binary).
#[must_use]
pub fn format_outcome(outcome: &RunOutcome) -> String {
    let mut s = String::new();
    match outcome {
        RunOutcome::Error { reason } => {
            writeln!(s, "\x1b[31merror:\x1b[0m {reason}").unwrap();
        }
        RunOutcome::Complete { report } => {
            writeln!(s, "spec: {}", report.spec_name).unwrap();
            writeln!(
                s,
                "\x1b[32mpassed:\x1b[0m {} \x1b[31mfailed:\x1b[0m {} \x1b[33mwarned:\x1b[0m {} \x1b[36mskipped:\x1b[0m {} (total {})",
                report.passed, report.failed, report.warned, report.skipped, report.total_cases
            )
            .unwrap();
        }
    }
    s
}

#[cfg(test)]
mod coverage_exit_tests {
    use super::*;
    use specgate_harness::{CaseLevel, CaseResult, CoverageReport};

    fn case(status: CaseStatus) -> CaseResult {
        CaseResult {
            name: "c".into(),
            status,
            level: CaseLevel::Must,
            source: None,
            expected: Vec::new(),
            traces: Vec::new(),
        }
    }

    fn report(percent: f64) -> CoverageReport {
        CoverageReport {
            lines_total: 100,
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            lines_covered: percent as u64,
            percent,
            files: Vec::new(),
        }
    }

    #[test]
    fn error_is_nonzero() {
        assert_eq!(coverage_exit_code(&CoverageOutcome::Error { reason: "x".into() }, None), 1);
    }

    #[test]
    fn measured_passing_without_threshold_is_zero() {
        let o = CoverageOutcome::Measured {
            results: vec![case(CaseStatus::Pass)],
            coverage: report(23.0),
        };
        assert_eq!(coverage_exit_code(&o, None), 0);
    }

    #[test]
    fn measured_below_threshold_is_nonzero() {
        let o = CoverageOutcome::Measured {
            results: vec![case(CaseStatus::Pass)],
            coverage: report(23.0),
        };
        assert_eq!(coverage_exit_code(&o, Some(90.0)), 1);
    }

    #[test]
    fn measured_at_or_above_threshold_is_zero() {
        let o = CoverageOutcome::Measured {
            results: vec![case(CaseStatus::Pass)],
            coverage: report(23.0),
        };
        assert_eq!(coverage_exit_code(&o, Some(10.0)), 0);
    }

    #[test]
    fn case_failure_dominates_threshold() {
        let o = CoverageOutcome::Measured {
            results: vec![case(CaseStatus::Fail)],
            coverage: report(100.0),
        };
        assert_eq!(coverage_exit_code(&o, Some(10.0)), 1);
    }

    #[test]
    fn unavailable_does_not_fail_on_threshold_but_respects_case_failures() {
        let ok = CoverageOutcome::Unavailable {
            results: vec![case(CaseStatus::Pass)],
            reason: "no llvm-tools".into(),
        };
        assert_eq!(coverage_exit_code(&ok, Some(99.0)), 0);
        let bad = CoverageOutcome::Unavailable {
            results: vec![case(CaseStatus::Fail)],
            reason: "no llvm-tools".into(),
        };
        assert_eq!(coverage_exit_code(&bad, None), 1);
    }
}
