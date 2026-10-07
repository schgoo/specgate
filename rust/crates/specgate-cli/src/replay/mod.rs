//! `specgate replay <capture-dir> <binding.yaml> --out <candidate.otlp.json>` —
//! statically link CTSC stimuli to a Rust candidate and replay them.
//!
//! # Examples
//! ```no_run
//! use specgate_cli::replay::{Paths, Request, replay};
//! let request = Request::builder(Paths::new("capture", "candidate.yaml", "candidate.json")).build()?;
//! let report = replay(request)?;
//! assert_eq!(report.output_path, std::path::Path::new("candidate.json"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use crate::system::{CommandEnvironment, Discovery, Execution, ProcessRequest};
use serde::{Deserialize, Serialize};
use specgate::__rt::Capture;
use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::replay::decode as decode_bundle;
use specgate_ctsc::replay::model::{Bundle as ReplayBundle, Input as ReplayInput, Type as ReplayType, Value as ReplayValue};
use specgate_discovery::output::SchemaLookup;
#[cfg(test)]
use specgate_discovery::registry::Field as RawField;
use specgate_discovery::registry::{Operation as RawOperation, Registry};
use specgate_discovery::schema::{Operation, Schema};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

// Capture bundle layout contract; changing these names breaks replay and golden compatibility.
fn manifest_file() -> &'static Path {
    Path::new("manifest.json")
}
// Canonical registry member of every capture bundle.
fn registry_file() -> &'static Path {
    Path::new("registry.ctsc.json")
}
// Canonical reference trace member of every capture bundle.
fn reference_file() -> &'static Path {
    Path::new("reference.otlp.json")
}
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "dyn",
];

mod error;
pub use error::Error;
use error::FailureStage as ErrorKind;
use error::failure;

fn encode_otlp(
    captures: impl AsRef<[Capture]>,
    metadata: &specgate_ctsc::capture::Metadata,
) -> Result<specgate_ctsc::capture::Encoding, Error> {
    let bytes = serde_json::to_vec(captures.as_ref())
        .map_err(|source| Error::wrap(ErrorKind::Publication, "failed to convert replay evidence", source))?;
    let captures: Vec<specgate_ctsc::capture::Capture> =
        serde_json::from_slice(&bytes).expect("Capture serialized successfully but violated the matching CTSC Capture representation");
    specgate_ctsc::capture::encode_candidate(&captures, metadata)
        .map_err(|source| Error::wrap(ErrorKind::Publication, source.to_string(), source))
}

#[cfg(test)]
mod limitation;
#[cfg(test)]
pub(crate) use limitation::{ReplayLimitation, classify};
mod request;
#[doc(inline)]
pub use request::{Paths, Report, Request, RequestBuilder};
mod model;
use model::{Candidate, Input, Link, Plan, PlannedOp, Scenario, Target};
mod facade;
#[cfg(any(test, feature = "test-util"))]
use facade::read_bundle;
#[doc(inline)]
pub use facade::{replay, replay_with};

/// Render a replay result using the stable CLI line protocol.
#[doc(hidden)]
#[must_use]
pub fn format_outcome(outcome: &Result<Report, Error>) -> String {
    match outcome {
        Ok(report) => format!(
            "Complete(component={}, scenarios={}, operations={}, plans={}, output={})\n",
            report.component_id(),
            report.scenarios,
            report.operations,
            report.plans,
            report.output_path.display()
        ),
        Err(error) => format!("Error({})\n", error.diagnostic()),
    }
}

mod execution;
pub(crate) use execution::Candidates;
use execution::{CandidatesInner, DiscoveryContext, DiscoveryInput};
mod output;
use output::write;
mod link;
use link::{build_plan, discover_candidate};
mod runner;
use runner::{execute_with, write_atomic};
#[cfg(test)]
mod tests;
