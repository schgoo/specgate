//! specgate-cli library: validate, run and extract commands used by the binary
//! and by the integration test suite.

pub mod extract;
pub mod run;
pub mod validate;

pub use extract::{ExtractOutcome, ExtractReport, extract};
pub use run::{RunOutcome, RunReport, run};
pub use validate::{Severity, ValidateOutcome, ValidationFinding, ValidationReport, validate};
