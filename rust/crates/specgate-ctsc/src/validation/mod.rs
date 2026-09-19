//! Native CTSC 0.2 validation.

mod bundle;
mod linked;
mod model;
mod otlp;
mod registry;
mod trace;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub(crate) use linked::{canonical_typed_value, find_operation, validate_linked_model};
pub(crate) use model::{AnyValue, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceEvent, TraceSpan, TypeRef};
pub(crate) use registry::load_registry_set;
pub(crate) use trace::load_trace;

/// One stable validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// JSON-like document location.
    pub location: String,
    /// Actionable failure description.
    pub message: String,
}

/// Public result of validating one CTSC artifact or bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Validation level: `registry`, `trace`, `linked`, or `bundle`.
    pub level: String,
    /// Primary input path.
    pub artifact: String,
    /// Whether no issues were found.
    pub valid: bool,
    /// Stable issues in discovery order.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    fn new(level: &str, artifact: &Path, issues: Vec<ValidationIssue>) -> Self {
        Self {
            level: level.to_string(),
            artifact: artifact.display().to_string(),
            valid: issues.is_empty(),
            issues,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Loaded<T> {
    pub(crate) value: Option<T>,
    pub(crate) issues: Vec<ValidationIssue>,
}

/// Validate a root CTSC registry and optional explicitly supplied imports.
#[must_use]
pub fn validate_registry(root: &Path, imports: &[PathBuf]) -> ValidationReport {
    let result = load_registry_set(root, imports);
    ValidationReport::new("registry", root, result.issues)
}

/// Validate an OTLP JSON or JSONL artifact at Trace Core level.
#[must_use]
pub fn validate_trace(path: &Path) -> ValidationReport {
    let result = load_trace(path);
    ValidationReport::new("trace", path, result.issues)
}

/// Validate a trace against its exact root registry and imports.
#[must_use]
pub fn validate_linked(trace: &Path, root: &Path, imports: &[PathBuf]) -> ValidationReport {
    let registry = load_registry_set(root, imports);
    let parsed_trace = load_trace(trace);
    let mut issues = registry.issues;
    issues.extend(parsed_trace.issues);
    if issues.is_empty()
        && let (Some(registry), Some(parsed_trace)) = (registry.value.as_ref(), parsed_trace.value.as_ref())
    {
        validate_linked_model(parsed_trace, registry, &mut issues);
    }
    ValidationReport::new("linked", trace, issues)
}

/// Validate a complete `SpecGate` capture bundle without applying replay limits.
#[must_use]
pub fn validate_bundle(directory: &Path) -> ValidationReport {
    ValidationReport::new("bundle", directory, bundle::validate(directory))
}

pub(crate) fn issue(issues: &mut Vec<ValidationIssue>, location: impl Into<String>, message: impl Into<String>) {
    issues.push(ValidationIssue {
        location: location.into(),
        message: message.into(),
    });
}

pub(crate) fn located(path: &Path, location: &str) -> String {
    format!("{}:{location}", path.display())
}

pub(crate) fn sha256_digest(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes))
}

pub(crate) fn is_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
}

pub(crate) fn read_bytes(path: &Path, issues: &mut Vec<ValidationIssue>) -> Option<Vec<u8>> {
    std::fs::read(path)
        .map_err(|error| issue(issues, located(path, "$"), format!("failed to read file: {error}")))
        .ok()
}
