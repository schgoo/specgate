//! `specgate run <spec>` — wrap the harness and produce a RunReport.

use std::path::Path;

use specgate_annotations::spec_operation;
use specgate_harness::{CaseStatus, RunOutcome as HarnessOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunReport {
    pub spec_name: String,
    pub total_cases: i32,
    pub passed: i32,
    pub failed: i32,
    pub skipped: i32,
    pub warned: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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
                report.spec_name,
                report.total_cases,
                report.passed,
                report.failed,
                report.skipped,
                report.warned
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
                total_cases: results.len() as i32,
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
    v.get("name")?.as_str().map(|s| s.to_string())
}

/// Render a run outcome to a colored, human-readable string for the
/// terminal (used by the binary).
pub fn format_outcome(outcome: &RunOutcome) -> String {
    let mut s = String::new();
    match outcome {
        RunOutcome::Error { reason } => {
            s.push_str(&format!("\x1b[31merror:\x1b[0m {reason}\n"));
        }
        RunOutcome::Complete { report } => {
            s.push_str(&format!("spec: {}\n", report.spec_name));
            s.push_str(&format!(
                "\x1b[32mpassed:\x1b[0m {} \x1b[31mfailed:\x1b[0m {} \x1b[33mwarned:\x1b[0m {} \x1b[36mskipped:\x1b[0m {} (total {})\n",
                report.passed, report.failed, report.warned, report.skipped, report.total_cases
            ));
        }
    }
    s
}
