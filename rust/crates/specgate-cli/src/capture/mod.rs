//! `specgate capture <binding.yaml> --out <dir>` — capture passing Rust tests
//! as one deterministic native CTSC reference bundle.
//!
//! Build a validated [`CaptureRequest`] from [`CapturePaths`], then call [`capture`].
//! The report identifies the three published bundle artifacts; failures retain
//! stage classification and source chains through [`crate::CaptureError`].
//!
//! # Examples
//!
//! ```no_run
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use specgate_cli::{CapturePaths, CaptureRequest, capture};
//! let request = CaptureRequest::builder(CapturePaths {
//!     binding: "binding.yaml".into(), out: "capture".into(),
//! }).component("example.math").build()?;
//! let report = capture(request)?;
//! assert!(report.manifest_path.ends_with("manifest.json"));
//! # Ok(())
//! # }
//! ```

use crate::capture_error::CaptureErrorKind;
use crate::capture_failure::CaptureError;
use crate::system::{CommandEnvironment, Discovery, Execution, ProcessRequest};
use serde::Serialize;
use sha2::{Digest, Sha256};
use specgate::__rt::{Capture, Config, ConfigDeps, EnvConfig, FAILURE_MARKER, SpanId, TraceId};
use specgate::{ComponentId, SpecEvent, TargetName, spec_operation};
use specgate_ctsc::{capture as ctsc_capture, registry::encode_many as encode_registries};
use specgate_discovery::output::Target;
use specgate_discovery::registry::Registry;
use specgate_discovery::runner::cargo_bin;
use specgate_discovery::schema::normalize_registry;

use std::collections::BTreeSet;
use std::error::Error as _;
use std::path::{Path, PathBuf};

fn encode_otlp(captures: impl AsRef<[Capture]>, metadata: &ctsc_capture::Metadata) -> Result<ctsc_capture::Encoding, ContextError> {
    let bytes =
        serde_json::to_vec(captures.as_ref()).map_err(|error| ContextError::with_source("failed to convert captured evidence", error))?;
    let captures: Vec<ctsc_capture::Capture> =
        serde_json::from_slice(&bytes).map_err(|error| ContextError::with_source("failed to convert captured evidence", error))?;
    ctsc_capture::encode_reference(&captures, metadata).map_err(|error| ContextError::with_source(error.to_string(), error))
}

#[derive(Debug, Clone, Copy)]
struct ArtifactName(&'static str);
impl ArtifactName {
    #[cfg(test)]
    const fn as_str(self) -> &'static str {
        self.0
    }
}
impl AsRef<str> for ArtifactName {
    fn as_ref(&self) -> &str {
        self.0
    }
}
impl AsRef<Path> for ArtifactName {
    fn as_ref(&self) -> &Path {
        Path::new(self.0)
    }
}
impl std::fmt::Display for ArtifactName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl Serialize for ArtifactName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}
// These filenames define the capture bundle layout consumed by replay, validators,
// comparators, and CTSC goldens. Any change requires a coordinated format migration
// and regenerated golden artifacts.
const REGISTRY_FILE: ArtifactName = ArtifactName("registry.ctsc.json");
const TRACE_FILE: ArtifactName = ArtifactName("reference.otlp.json");
const MANIFEST_FILE: ArtifactName = ArtifactName("manifest.json");
// Capture readers use this discriminator to select the manifest schema;
// changing it requires a coordinated reader migration.
const MANIFEST_FORMAT: &str = "specgate.capture-manifest";
// Capture readers validate this schema version before interpreting any bundle
// paths or digests; changing it requires a coordinated reader migration.
const FORMAT_VERSION: &str = "0.1.0";
// Registry artifacts produced by capture use this externally validated schema
// version; changing it requires coordinated CTSC readers and golden updates.
const REGISTRY_VERSION: &str = "0.1.0";
// Capture registries use the CTSC registry URN namespace consumed by bundle validators and replay.
const REGISTRY_PREFIX: &str = "urn:ctsc:registry:";
// Valid, nonzero hexadecimal IDs make isolated captures deterministic. They
// are rebased by the CTSC encoder before becoming bundle identifiers.
fn trace_id() -> TraceId {
    TraceId::try_from("11111111111111111111111111111111").expect("hard-coded deterministic capture trace ID must be valid")
}
fn run_span() -> SpanId {
    SpanId::try_from("1111111111111101").expect("hard-coded deterministic capture run span ID must be valid")
}
fn scenario_span() -> SpanId {
    SpanId::try_from("1111111111111102").expect("hard-coded deterministic capture scenario span ID must be valid")
}
// Epoch zero removes wall-clock variation from isolated capture artifacts.
const START_TIME: i64 = 0;
// One-nanosecond logical ticks preserve event order without wall-clock input.
const CLOCK_STEP: i64 = 1;
// Keep enough trailing output to include a typical panic and backtrace header
// while bounding one failed test's contribution to CLI diagnostics.
const TAIL_LINES: usize = 12;
// Bound captured text independently of line count because compiler and panic
// messages can contain unusually long source or path lines. Twelve lines at
// 100 characters each cover the normal panic header while imposing a 1.2 KiB bound.
const TAIL_CHARS: usize = 1_200;
// Fixed-width indices keep sidecar filenames lexically ordered in execution
// order. Eight decimal digits support up to 100 million test processes without
// disturbing lexical order; changing the width changes scratch filenames only.
const INDEX_WIDTH: usize = 8;
// Fold both halves of epoch seconds before mixing a bounded scratch suffix;
// keeping the suffix at u32 width avoids Windows build-path overflow.
// Test binaries read this environment variable before lazily starting capture; changing
// this protocol key would silently disable capture in already-built instrumented binaries.
const CAPTURE_ENV: &str = "SPECGATE_NATIVE_CAPTURE";

mod request;
pub use request::{CapturePaths, CaptureReport, CaptureRequest, CaptureRequestBuilder};
mod failure;
use failure::{ContextError, public_error};
mod model;
use model::{
    CaptureManifest, IsolatedTest, Language, Reference, RegistryDto, RegistryId, Scenarios, TargetDto, TestBinary, Tool, Version,
    WireDigest,
};
mod facade;
#[cfg(any(test, feature = "test-util"))]
#[cfg_attr(
    all(feature = "test-util", not(test)),
    expect(unused_imports, reason = "feature-gated fixture support")
)]
pub(crate) use facade::{BundleRequest, capture_strict, discover};
pub use facade::{capture, capture_with};
mod encoding;
use encoding::encode_bundle;
mod filter;
use filter::{select_component, validate_setups};
mod execution;
use execution::{
    ExecutedTests, artifact_path, build_binaries, enumerate_tests, reject_failures, run_tests, scratch_dir, sha256_digest, write_artifact,
};
#[cfg(test)]
mod tests;
