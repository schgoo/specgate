//! `SpecGate` types — the typed data model for specs, bindings, and run
//! results, shared by the CLI and the harness so structure is defined once.
//!
//! - `spec_document` — the parsed `.spec.yaml` representation (`SpecDocument`,
//!   `SpecCase`, `TestStep`, ...) plus `validate_spec_document` for
//!   schema-level validation.
//! - `binding_file` — the parsed binding YAML (`BindingFile`, `BindingTarget`).
//! - `report` — run-result types (`RunOutcome`, `RunReport`, `CaseResult`,
//!   `CaseStatus`, `RunError`).
//!
//! These are plain serde-parsed data types with no behavior of their own.

mod binding_file;
mod report;
mod spec_document;

pub use binding_file::{BindingFile, BindingTarget, BindingTargetOutputs};
pub use report::{CaseResult, CaseStatus, RunError, RunOutcome, RunReport};
pub use spec_document::{BindingDecl, BindingEntry, Postcondition, SpecCase, SpecDocument, TestStep, validate_spec_document};
