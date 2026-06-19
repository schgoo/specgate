//! Public types for the harness output.
//!
//! These mirror the `types:` block of `specs/specgate.harness.spec.yaml`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TraceEvent {
    Event { name: String, value: String },
    Run { operation: String },
}

/// Structured expected-trace assertion. Mirrors the `Assertion` oneof in
/// the v0.4.0 spec: `Event { name, value }`, `$run { operation }`,
/// `$unordered { items }`, `$anywhere { items }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    Event { name: String, value: String },
    Run { operation: String },
    Unordered { items: Vec<Assertion> },
    Anywhere { items: Vec<Assertion> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseStatus {
    Pass,
    Fail,
    Skip,
    Warn,
}

/// Normative strength of a case. Affects what happens when the operation
/// or setup the case references is missing from the source annotations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseLevel {
    Must,
    Should,
    May,
}

impl Default for CaseLevel {
    fn default() -> Self {
        CaseLevel::Must
    }
}

/// Free-form provenance metadata threaded through from the spec.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    pub assertion_ids: Vec<String>,
    pub spec: String,
    pub section: String,
}

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub status: CaseStatus,
    pub level: CaseLevel,
    pub source: Option<Source>,
    pub expected: Vec<Assertion>,
    pub traces: Vec<TraceEvent>,
}

#[derive(Debug, Clone)]
pub enum RunOutcome {
    Complete { results: Vec<CaseResult> },
    Error { reason: String },
}

impl RunOutcome {
    pub fn is_complete(&self) -> bool {
        matches!(self, RunOutcome::Complete { .. })
    }
}

impl CaseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaseStatus::Pass => "pass",
            CaseStatus::Fail => "fail",
            CaseStatus::Skip => "skip",
            CaseStatus::Warn => "warn",
        }
    }
}

impl std::fmt::Display for RunOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunOutcome::Complete { results } => write!(f, "Complete({} results)", results.len()),
            RunOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

impl CaseLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaseLevel::Must => "must",
            CaseLevel::Should => "should",
            CaseLevel::May => "may",
        }
    }
}
