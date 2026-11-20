//! Native registry declaration comparison and deterministic CTSC Strict trace comparison.
//!
//! Inputs are fully validated before comparison. Reports separate malformed or unsupported
//! input diagnostics from behavioral mismatches; `equivalent` is true only when all three
//! collections are empty. Strict comparison ignores producer IDs and timing while preserving
//! semantic scenario, operation, input, observation, completion, and parallel-branch meaning.
//!
//! [`compare_registry_documents`] separately compares two independently validated
//! native registry sets. Signature differences do not change Strict trace equality
//! or establish full API equivalence. This byte API is not CLI/report integration.
//!
//! # Example
//! ```no_run
//! use specgate_ctsc::comparison::{SystemReader, compare_with};
//! let report = compare_with(
//!     "reference.otlp.json",
//!     "candidate.otlp.json",
//!     None::<&str>,
//!     Vec::<std::path::PathBuf>::new(),
//!     &SystemReader::system(),
//! );
//! if !report.equivalent {
//!     eprintln!("{} mismatches", report.mismatches.len());
//! }
//! ```

mod engine;
mod loading;
mod reading;
mod report;
mod signatures;

pub use engine::compare_with;
pub(crate) use engine::{compare_bytes, compare_paths};
pub use loading::{DocumentReader, LoadError, SystemReader};
pub use report::{
    ComparisonDiagnostic, ComparisonMismatch, ComparisonReport, ComparisonReportBuilder, DiagnosticLocation, DiagnosticText, ReportPaths,
    SemanticPath, SemanticValue, ValidationDiagnostic,
};
pub use signatures::{RegistryComparisonReport, SignatureDifference, compare_registry_documents};

/// CTSC Strict policy identifier serialized in every comparison report.
///
/// Report consumers dispatch on this CTSC contract value. Changing it requires
/// coordinated contract review and compatibility updates in every consumer.
pub const POLICY_ID: &str = "ctsc.strict";
/// CTSC Strict Comparison contract version.
///
/// `0.2.0` identifies the current authoritative Strict policy revision consumed
/// by serialized reports. Semantic comparison changes require coordinated CTSC
/// contract review, consumer compatibility updates, and a version bump here.
pub const POLICY_VERSION: &str = "0.2.0";
