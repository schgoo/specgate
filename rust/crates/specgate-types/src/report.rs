use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CaseStatus {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "fail")]
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CaseResult {
    pub name: String,
    pub status: CaseStatus,
    pub duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traces_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traces_match: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunReport {
    pub spec_name: String,
    pub binding: String,
    pub timestamp: String,
    pub duration_ms: i64,
    pub results: Vec<CaseResult>,
    pub passed: usize,
    pub failed: usize,
    pub total: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunError {
    SpecNotFound { path: String },
    SpecInvalid { detail: String },
    BindingNotFound { binding: String },
    BackendNotFound { language: String },
    GenerateFailed { detail: String },
    BuildFailed { detail: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RunOutcome {
    Complete { report: RunReport },
    Error { error: RunError },
}
