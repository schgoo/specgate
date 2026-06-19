//! Public types for the harness output.
//!
//! These mirror the `types:` block of `specs/specgate.harness.spec.yaml`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub use specgate_runtime::{TraceEvent, Value};

/// Either an exact-`Value` assertion or a $-operator matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssertValue {
    Exact(Value),
    Matcher(Matcher),
}

impl From<Value> for AssertValue {
    fn from(v: Value) -> Self { AssertValue::Exact(v) }
}
impl From<&str> for AssertValue {
    fn from(s: &str) -> Self { AssertValue::Exact(Value::String(s.to_string())) }
}
impl From<String> for AssertValue {
    fn from(s: String) -> Self { AssertValue::Exact(Value::String(s)) }
}
impl From<i64> for AssertValue {
    fn from(i: i64) -> Self { AssertValue::Exact(Value::Integer(i)) }
}
impl From<i32> for AssertValue {
    fn from(i: i32) -> Self { AssertValue::Exact(Value::Integer(i as i64)) }
}
impl From<bool> for AssertValue {
    fn from(b: bool) -> Self { AssertValue::Exact(Value::Bool(b)) }
}

/// Structured matcher for assertion values. Composite is used when a single
/// assertion-payload mapping contains multiple `$ops` — all must pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Matcher {
    Eq(Value),
    Size(usize),
    Contains(Box<AnyArg>),
    ContainsAll(Vec<Value>),
    Excludes(Vec<Value>),
    Match(BTreeMap<String, Value>),
    Exists(bool),
    Any(Box<AnyArg>),
    Every(Box<AnyArg>),
    Type(String),
    Matches(String),
    Not(Box<Matcher>),
    Gt(Value),
    Gte(Value),
    Lt(Value),
    Lte(Value),
    Composite(Vec<Matcher>),
}

/// Argument for `$any` — either a concrete value to compare against, or a
/// nested matcher (`{ $matches: "..." }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnyArg {
    Value(Value),
    Matcher(Matcher),
}

/// Structured expected-trace assertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Assertion {
    Event { name: String, value: AssertValue },
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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

