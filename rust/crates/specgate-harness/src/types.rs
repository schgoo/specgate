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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaseStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone)]
pub struct CaseResult {
    pub name: String,
    pub status: CaseStatus,
    pub expected: Vec<std::collections::BTreeMap<String, String>>,
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
        }
    }
}
