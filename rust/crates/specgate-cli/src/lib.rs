//! specgate-cli library: validate and run commands used by the binary
//! and by the integration test suite.

pub mod run;
pub mod validate;

pub use run::{RunOutcome, RunReport, run};
pub use validate::{Severity, ValidateOutcome, ValidationFinding, ValidationReport, validate};
