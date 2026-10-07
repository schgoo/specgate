//! Public CTSC Strict report values and their construction API.
//!
//! Report text wrappers deliberately accept every string, including empty and
//! malformed spellings: comparison must preserve observed diagnostics and
//! sentinel values rather than reject the evidence it is reporting.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

macro_rules! report_text {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            /// Borrow the report spelling.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                self.as_str()
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
        impl PartialEq<str> for $name {
            fn eq(&self, other: &str) -> bool {
                self.as_str() == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str() == other
            }
        }
    };
}
report_text!(SemanticPath, "Stable semantic location in a strict comparison.");
report_text!(SemanticValue, "Canonical semantic value rendered in a mismatch.");
report_text!(DiagnosticLocation, "Artifact location rendered in a validation diagnostic.");
report_text!(DiagnosticText, "Actionable comparison or validation diagnostic text.");

/// One deterministic behavioral mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "comparison reports are an exhaustively serialized CTSC result protocol"
)]
pub struct ComparisonMismatch {
    /// Semantic location.
    pub path: SemanticPath,
    /// Reference semantic value.
    pub expected: SemanticValue,
    /// Candidate semantic value.
    pub actual: SemanticValue,
}

/// Unsupported or ambiguous input under CTSC Strict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "comparison reports are an exhaustively serialized CTSC result protocol"
)]
pub struct ComparisonDiagnostic {
    /// Semantic location.
    pub path: SemanticPath,
    /// Failure description.
    pub message: DiagnosticText,
}

/// Serialized validation diagnostic copied into a comparison report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(clippy::exhaustive_structs, reason = "diagnostics support direct field access by callers")]
pub struct ValidationDiagnostic {
    /// Prefixed artifact location.
    pub location: DiagnosticLocation,
    /// Actionable validation message.
    pub message: DiagnosticText,
}

/// Typed result of one CTSC Strict comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[expect(
    clippy::exhaustive_structs,
    reason = "comparison reports are an exhaustively serialized CTSC result protocol"
)]
pub struct ComparisonReport {
    /// Policy identifier.
    pub policy: String,
    /// Policy version.
    pub policy_version: String,
    /// Reference artifact path.
    pub reference: PathBuf,
    /// Candidate artifact path.
    pub candidate: PathBuf,
    /// True only when validation succeeds and no differences exist.
    pub equivalent: bool,
    /// Trace Core, Registry, or Linked validation failures.
    pub validation_failures: Vec<ValidationDiagnostic>,
    /// Strict-policy unsupported or ambiguous conditions.
    pub errors: Vec<ComparisonDiagnostic>,
    /// Ordered semantic differences.
    pub mismatches: Vec<ComparisonMismatch>,
}

/// Required artifact paths for constructing a comparison report.
#[derive(Debug)]
#[expect(clippy::exhaustive_structs, reason = "builder dependencies support direct construction by callers")]
pub struct ReportPaths {
    /// Reference artifact path.
    pub reference: PathBuf,
    /// Candidate artifact path.
    pub candidate: PathBuf,
}

/// Chainable construction for externally produced comparison reports.
///
/// `equivalent` is derived during [`ReportBuilder::build`]: it is true only
/// when validation failures, policy errors, and mismatches are all empty.
///
/// # Examples
/// ```
/// use specgate_ctsc::comparison::{ComparisonReport, ReportPaths};
/// let report = ComparisonReport::builder(ReportPaths {
///     reference: "reference.json".into(),
///     candidate: "candidate.json".into(),
/// }).build();
/// assert!(report.equivalent);
/// ```
#[derive(Debug)]
#[must_use]
pub struct ReportBuilder {
    paths: ReportPaths,
    validation_failures: Vec<ValidationDiagnostic>,
    errors: Vec<ComparisonDiagnostic>,
    mismatches: Vec<ComparisonMismatch>,
}

impl ComparisonReport {
    /// Begin a report with required artifact paths and empty diagnostics.
    pub fn builder(paths: impl Into<ReportPaths>) -> ReportBuilder {
        ReportBuilder {
            paths: paths.into(),
            validation_failures: Vec::new(),
            errors: Vec::new(),
            mismatches: Vec::new(),
        }
    }
}

impl ReportBuilder {
    /// Set validation failures.
    pub fn validation_failures(mut self, value: Vec<ValidationDiagnostic>) -> Self {
        self.validation_failures = value;
        self
    }
    /// Set strict-policy errors.
    pub fn errors(mut self, value: Vec<ComparisonDiagnostic>) -> Self {
        self.errors = value;
        self
    }
    /// Set semantic mismatches.
    pub fn mismatches(mut self, value: Vec<ComparisonMismatch>) -> Self {
        self.mismatches = value;
        self
    }
    /// Build the exhaustively serialized CTSC report with derived equivalence.
    #[must_use]
    pub fn build(self) -> ComparisonReport {
        let equivalent = self.validation_failures.is_empty() && self.errors.is_empty() && self.mismatches.is_empty();
        ComparisonReport {
            policy: super::POLICY_ID.to_owned(),
            policy_version: super::POLICY_VERSION.to_owned(),
            reference: self.paths.reference,
            candidate: self.paths.candidate,
            equivalent,
            validation_failures: self.validation_failures,
            errors: self.errors,
            mismatches: self.mismatches,
        }
    }
}
