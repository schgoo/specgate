//! specgate-cli library: validate, run and extract commands used by the binary
//! and by the integration test suite.

// The crate-root default component for all annotated items in this crate's
// submodules (extract/run/validate). Submodules reference the generated
// `crate::__SPECGATE_COMPONENT` constant.
specgate::spec_component!("specgate.cli");

pub mod extract;
pub mod run;
pub mod validate;

pub use extract::{ExtractOutcome, ExtractReport, extract};
pub use run::{RunOutcome, RunReport, run};
pub use validate::{Severity, ValidateOutcome, ValidationFinding, ValidationReport, validate};
