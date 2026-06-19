//! specgate-cli library: validate and run commands used by the binary
//! and by the integration test suite.

pub mod run;
pub mod validate;

pub use run::{run, RunOutcome, RunReport};
pub use validate::{validate, ValidateOutcome, ValidationFinding, ValidationReport, Severity};
