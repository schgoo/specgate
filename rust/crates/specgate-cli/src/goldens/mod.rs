#![cfg(any(test, feature = "test-util"))]
//! CTSC golden matrix harness.
//!
//! The checked-in matrix at `test/goldens/ctsc/matrix.json` is the reviewable
//! configuration; every artifact beside it is regenerated product output. The
//! harness discovers, captures, and validates the whole fixture corpus in one
//! pass, then either rewrites the goldens (`update`) or byte-compares fresh
//! output against them (`check`).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use specgate_ctsc::compare;
use specgate_ctsc::registry::encode as encode_registry;
use specgate_ctsc::validation::{validate_bundle, validate_linked, validate_registry, validate_trace};
use specgate_discovery::binding::resolve_target;
use specgate_discovery::registry::Registry;
use specgate_discovery::runner::cargo_bin;
use specgate_discovery::schema::normalize_registry;
use specgate_discovery::{discover_batch, output::SchemaLookup};

use crate::capture_impl::{BundleRequest, capture_strict, discover};
use crate::replay::{Candidates, classify};
use specgate::ComponentId;

fn matrix_file() -> &'static Path {
    Path::new("matrix.json")
}
fn registry_file() -> &'static Path {
    Path::new("registry.ctsc.json")
}
// Negative golden readers key off this format/version pair. Changing either
// requires coordinated artifact regeneration and reader migration.
const ERROR_FORMAT: &str = "specgate.ctsc-golden-error";
const ERROR_VERSION: &str = "0.1.0";
fn trace_file() -> &'static Path {
    Path::new("reference.otlp.json")
}
fn manifest_file() -> &'static Path {
    Path::new("manifest.json")
}
fn error_file() -> &'static Path {
    Path::new("error.json")
}
// Generated Registry Core documents and replay linkage share this version;
// changing it requires regenerating every CTSC golden artifact.
const REGISTRY_VERSION: &str = "0.1.0";
// The hand-authored matrix format gates interpretation of every generated
// artifact; changing either value requires a coordinated harness migration.
const MATRIX_FORMAT: &str = "specgate.ctsc-golden-matrix";
const MATRIX_VERSION: &str = "0.1.0";
// Short strings are too collision-prone to identify a machine-specific root;
// bare drive roots such as `C:\` are deliberately ignored by this heuristic.
const MARKER_MIN: usize = 4;

#[cfg(test)]
const CLASS_COMPONENT: &str = "implementation-component";

/// The limitation code a row declares when an async setup prevents capture
/// and the component can therefore only be discovered.
const ASYNC_LIMITATION: &str = "async-capture-unsupported";
// These are the stable planner limitations accepted by `unsupported` replay
// rows. Each category names a capability deliberately outside native replay:
// structured-value projection, setup injection, instance methods, async
// execution, or a non-Rust target. Changes must update matrix rows and the
// generated golden artifacts through `just ctsc-goldens-update`.
const REPLAY_FAILURES: &[&str] = &[
    "structured-value",
    "setup-backed-operation",
    "method-operation",
    "async-operation",
    "unsupported-language",
];

#[cfg(any(test, feature = "test-util"))]
mod support;
use support::{Attribution, CompileError, GoldenError};
mod schema;
use schema::*;
mod matrix;
use matrix::*;
mod source;
use source::*;
mod generation;
use generation::*;
mod diagnostics;
use diagnostics::*;
mod validation;
use validation::*;
mod replay;
use replay::*;
mod driver;
use driver::*;
#[cfg(test)]
mod tests;
