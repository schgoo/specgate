//! CTSC capture, registry, replay, validation, and strict comparison.
//!
//! The semantic modules are the only public paths: [`capture`] converts
//! crate-owned evidence to deterministic OTLP JSON, [`registry`] encodes
//! normalized schemas, [`replay`] verifies bundle bytes, and [`validation`]
//! plus [`comparison`] inspect artifacts without I/O when byte APIs are used.
//!
//! ```no_run
//! let report = specgate_ctsc::compare("reference.json", "candidate.json", None::<&std::path::Path>, &[]);
//! println!("{}", report.equivalent);
//! ```
//!
//! Registry encoding returns opaque contextual errors instead of panicking:
//! ```
//! let schema = specgate_ctsc::registry::Schema::new("not JSON");
//! let error = specgate_ctsc::registry::encode("example", "1", schema).unwrap_err();
//! assert!(!error.to_string().is_empty());
//! ```
//! Byte validation is pure and reports all discovered issues:
//! ```
//! use specgate_ctsc::validation::{DocumentBytes, bytes::validate_trace};
//! let input = DocumentBytes::new(std::path::Path::new("trace.json"), b"{}");
//! assert!(!validate_trace(input).valid);
//! ```
//! Replay decoding verifies manifest digests and linkage before returning instructions:
//! ```
//! assert!(specgate_ctsc::replay::decode(b"{}", b"{}", b"{}").is_err());
//! ```
//! Capture encoding requires at least one completed scenario and reports invalid construction:
//! ```
//! use specgate_ctsc::capture::{Metadata, Registry, Target, encode_reference};
//! let metadata = Metadata::new("1", Target::new("target", "rust"), Registry::new("id", "1", "sha256:x"));
//! assert!(encode_reference([], &metadata).is_err());
//! ```

/// Compare two local CTSC traces with fixed `ctsc.strict/0.1.0` semantics.
///
/// Inputs are validated before semantic comparison. Invalid artifacts populate
/// [`comparison::ComparisonReport::validation_failures`] and are never reported
/// as target mismatches. Supplying a registry enables Linked value semantics.
///
/// # Examples
///
/// ```no_run
/// let report = specgate_ctsc::compare(
///     "reference.otlp.json", "candidate.otlp.json", None::<&std::path::Path>, &[],
/// );
/// println!("equivalent: {}", report.equivalent);
/// ```
#[must_use]
pub fn compare(
    reference: impl AsRef<std::path::Path>,
    candidate: impl AsRef<std::path::Path>,
    registry: Option<impl AsRef<std::path::Path>>,
    imports: impl AsRef<[std::path::PathBuf]>,
) -> comparison::ComparisonReport {
    comparison::compare_paths(reference, candidate, registry, imports)
}

/// Compare named in-memory CTSC documents without filesystem access.
///
/// Inputs receive the same Trace Core, Registry, Linked, and Strict-policy
/// validation as [`compare`]. Read failures are absent because callers own the
/// bytes; malformed documents are returned in `validation_failures`.
///
/// # Examples
/// ```
/// use specgate_ctsc::{compare_documents, validation::DocumentBytes};
/// use std::path::Path;
/// let trace = DocumentBytes::new(Path::new("trace.json"), br#"{"resourceSpans":[]}"#);
/// let report = compare_documents(trace, trace, None, &[]);
/// assert!(!report.equivalent);
/// ```
#[must_use]
pub fn compare_documents<'a>(
    reference: validation::DocumentBytes<'a>,
    candidate: validation::DocumentBytes<'a>,
    registry: Option<validation::DocumentBytes<'a>>,
    imports: impl AsRef<[validation::DocumentBytes<'a>]>,
) -> comparison::ComparisonReport {
    comparison::compare_bytes(reference, candidate, registry, imports.as_ref())
}

/// Compare paths through a user-supplied loading boundary.
///
/// The reader controls both byte loading and canonical import identity. Read
/// failures become ordered `validation_failures`; malformed loaded documents
/// receive the same Trace Core, Registry, Linked, and Strict-policy validation
/// as [`compare`]. This boundary supports deterministic failure tests and custom
/// stores without changing comparison semantics.
///
/// # Examples
/// ```
/// use specgate_ctsc::{compare_reader, comparison::{LoadError, DocumentReader}};
/// use std::path::{Path, PathBuf};
/// struct Denied;
/// impl DocumentReader for Denied {
///     fn read(&self, path: &Path) -> Result<Vec<u8>, LoadError> {
///         Err(LoadError::reading(path, std::io::ErrorKind::PermissionDenied.into()))
///     }
///     fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError> {
///         Ok(path.to_path_buf())
///     }
/// }
/// let report = compare_reader("reference.json", "candidate.json", None::<&Path>, &[], &Denied);
/// assert_eq!(report.validation_failures.len(), 2);
/// ```
#[must_use]
pub fn compare_reader(
    reference: impl AsRef<std::path::Path>,
    candidate: impl AsRef<std::path::Path>,
    registry: Option<impl AsRef<std::path::Path>>,
    imports: impl AsRef<[std::path::PathBuf]>,
    reader: &impl comparison::DocumentReader,
) -> comparison::ComparisonReport {
    comparison::compare_with(reference, candidate, registry, imports, reader)
}

/// Capture builders and deterministic native-to-OTLP encoding.
pub mod capture;
/// Strict comparison reports, loading boundaries, and policy constants.
pub mod comparison;
/// Deterministic registry encoding from discovery schemas.
pub mod registry;
/// Validated replay plans decoded from linked traces and registries.
pub mod replay;
/// Registry, Trace Core, Linked, and capture-bundle validators.
pub mod validation;
