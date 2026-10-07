//! Validate CTSC registry, trace, linked, and capture-bundle artifacts.
//!
//! Path-oriented functions read local artifacts. Matching byte-oriented
//! functions accept [`DocumentBytes`] and perform the same validation without
//! filesystem access.
//!
//! # Examples
//!
//! ```
//! use specgate_ctsc::validation::{DocumentBytes, ValidationLevel, bytes::validate_trace};
//! use std::path::Path;
//!
//! let report = validate_trace(DocumentBytes::new(
//!     Path::new("empty.otlp.json"),
//!     br#"{"resourceSpans":[]}"#,
//! ));
//! assert_eq!(report.level, ValidationLevel::Trace);
//! assert!(!report.valid);
//! ```

mod bundle;
mod linked;
mod model;
mod otlp;
pub(crate) mod registry;
pub(crate) mod trace;

use std::fmt;
use std::path::{Path, PathBuf};

pub(crate) use linked::{canonical_value, check_linked, find_operation};
pub(crate) use model::{AnyValue, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceEvent, TraceSpan, TypeRef};
pub(crate) use registry::load_set;
pub(crate) use trace::{load_bytes, load_trace};

/// One named in-memory artifact used by byte-oriented validation APIs.
#[derive(Debug, Clone, Copy)]
#[expect(clippy::exhaustive_structs, reason = "byte-oriented validation input is a complete borrowing DTO")]
pub struct DocumentBytes<'a> {
    /// Logical path used for format selection and diagnostics.
    pub path: &'a Path,
    /// Exact artifact bytes.
    pub bytes: &'a [u8],
}

impl<'a> DocumentBytes<'a> {
    /// Create an in-memory artifact with a diagnostic path.
    #[must_use]
    pub const fn new(path: &'a Path, bytes: &'a [u8]) -> Self {
        Self { path, bytes }
    }
}

/// The three exact documents that form an in-memory capture bundle.
#[derive(Debug, Clone, Copy)]
#[expect(clippy::exhaustive_structs, reason = "capture bundle input is a complete three-document DTO")]
pub struct BundleBytes<'a> {
    /// Capture manifest bytes and their logical path.
    pub manifest: DocumentBytes<'a>,
    /// Registry bytes and their logical path.
    pub registry: DocumentBytes<'a>,
    /// Reference trace bytes and their logical path.
    pub trace: DocumentBytes<'a>,
}

impl<'a> BundleBytes<'a> {
    /// Group the three named files of a capture bundle.
    #[must_use]
    pub const fn new(manifest: DocumentBytes<'a>, registry: DocumentBytes<'a>, trace: DocumentBytes<'a>) -> Self {
        Self { manifest, registry, trace }
    }
}

/// One stable validation diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "validation reports are an exhaustively serialized CTSC result protocol"
)]
pub struct ValidationIssue {
    /// JSON-like document location.
    pub location: String,
    /// Actionable failure description.
    pub message: String,
}

/// Validation contract applied to an artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValidationLevel {
    /// Registry document validation.
    Registry,
    /// Trace Core validation.
    Trace,
    /// Registry-linked trace validation.
    Linked,
    /// Complete capture-bundle validation.
    Bundle,
}
impl fmt::Display for ValidationLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Registry => "registry",
            Self::Trace => "trace",
            Self::Linked => "linked",
            Self::Bundle => "bundle",
        })
    }
}

/// Public result of validating one CTSC artifact or bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "validation reports are an exhaustively serialized CTSC result protocol"
)]
pub struct ValidationReport {
    /// Validation contract that was applied.
    pub level: ValidationLevel,
    /// Primary input path, retained as an operating-system path.
    pub artifact: PathBuf,
    /// Whether no issues were found.
    pub valid: bool,
    /// Stable issues in discovery order.
    pub issues: Vec<ValidationIssue>,
}
impl ValidationReport {
    fn new(level: ValidationLevel, artifact: &Path, mut issues: Vec<ValidationIssue>) -> Self {
        issues.shrink_to_fit();
        Self {
            level,
            artifact: artifact.to_path_buf(),
            valid: issues.is_empty(),
            issues,
        }
    }
}

#[derive(Debug)]
pub(crate) struct Loaded<T> {
    pub(crate) value: Option<T>,
    pub(crate) issues: Vec<ValidationIssue>,
}

/// Validate a root CTSC registry and optional explicitly supplied imports.
#[must_use]
pub fn validate_registry(root: impl AsRef<Path>, imports: impl AsRef<[PathBuf]>) -> ValidationReport {
    let root = root.as_ref();
    ValidationReport::new(ValidationLevel::Registry, root, load_set(root, imports.as_ref()).issues)
}

/// Validate an OTLP JSON or JSONL artifact at Trace Core level.
#[must_use]
pub fn validate_trace(path: impl AsRef<Path>) -> ValidationReport {
    let path = path.as_ref();
    ValidationReport::new(ValidationLevel::Trace, path, load_trace(path).issues)
}

/// Validate a trace against its exact root registry and imports.
#[must_use]
pub fn validate_linked(trace: impl AsRef<Path>, root: impl AsRef<Path>, imports: impl AsRef<[PathBuf]>) -> ValidationReport {
    let trace = trace.as_ref();
    let root = root.as_ref();
    let registry = load_set(root, imports.as_ref());
    let parsed = load_trace(trace);
    let mut issues = registry.issues;
    issues.extend(parsed.issues);
    if issues.is_empty()
        && let (Some(registry), Some(parsed)) = (registry.value.as_ref(), parsed.value.as_ref())
    {
        check_linked(parsed, registry, &mut issues);
    }
    ValidationReport::new(ValidationLevel::Linked, trace, issues)
}

/// Validate a complete `SpecGate` capture bundle without applying replay limits.
#[must_use]
pub fn validate_bundle(directory: impl AsRef<Path>) -> ValidationReport {
    let directory = directory.as_ref();
    ValidationReport::new(ValidationLevel::Bundle, directory, bundle::validate(directory))
}

pub mod reader {
    //! Path validators using a caller-supplied document-loading boundary.
    //!
    //! # Examples
    //!
    //! ```no_run
    //! use specgate_ctsc::comparison::SystemReader;
    //! use specgate_ctsc::validation::reader::validate_trace;
    //!
    //! let report = validate_trace("trace.otlp.json", &SystemReader::system());
    //! assert_eq!(report.artifact.to_string_lossy(), "trace.otlp.json");
    //! ```

    use super::{Path, PathBuf, ValidationLevel, ValidationReport, bundle, check_linked, registry, trace};
    use crate::comparison::DocumentReader;

    /// Validate a root registry and imports through `reader`.
    #[must_use]
    pub fn validate_registry(root: impl AsRef<Path>, imports: impl AsRef<[PathBuf]>, reader: &impl DocumentReader) -> ValidationReport {
        let root = root.as_ref();
        ValidationReport::new(
            ValidationLevel::Registry,
            root,
            registry::load_reader(root, imports.as_ref(), reader).issues,
        )
    }

    /// Validate a trace through `reader`.
    #[must_use]
    pub fn validate_trace(path: impl AsRef<Path>, reader: &impl DocumentReader) -> ValidationReport {
        let path = path.as_ref();
        ValidationReport::new(ValidationLevel::Trace, path, trace::load_from(path, reader).issues)
    }

    /// Validate a linked trace and registry through `reader`.
    #[must_use]
    pub fn validate_linked(
        trace_path: impl AsRef<Path>,
        root: impl AsRef<Path>,
        imports: impl AsRef<[PathBuf]>,
        reader: &impl DocumentReader,
    ) -> ValidationReport {
        let trace_path = trace_path.as_ref();
        let registry = registry::load_reader(root.as_ref(), imports.as_ref(), reader);
        let parsed = trace::load_from(trace_path, reader);
        let mut issues = registry.issues;
        issues.extend(parsed.issues);
        if issues.is_empty()
            && let (Some(registry), Some(parsed)) = (registry.value.as_ref(), parsed.value.as_ref())
        {
            check_linked(parsed, registry, &mut issues);
        }
        ValidationReport::new(ValidationLevel::Linked, trace_path, issues)
    }

    /// Validate a capture bundle through `reader`.
    #[must_use]
    pub fn validate_bundle(directory: impl AsRef<Path>, reader: &impl DocumentReader) -> ValidationReport {
        let directory = directory.as_ref();
        ValidationReport::new(ValidationLevel::Bundle, directory, bundle::validate_read(directory, reader))
    }
}

pub mod bytes {
    //! Sans-I/O, pure byte-oriented counterparts to the path validators.
    //!
    //! Use these functions for already-loaded artifacts. They perform no filesystem access.
    //!
    //! # Examples
    //!
    //! ```
    //! use specgate_ctsc::validation::{BundleBytes, DocumentBytes, bytes};
    //! use std::path::Path;
    //!
    //! let registry = DocumentBytes::new(Path::new("registry.ctsc.json"), br#"{}"#);
    //! let trace = DocumentBytes::new(Path::new("reference.otlp.json"), br#"{"resourceSpans":[]}"#);
    //! let manifest = DocumentBytes::new(Path::new("capture.ctsc.json"), br#"{}"#);
    //!
    //! assert!(!bytes::validate_registry(registry, []).valid);
    //! assert!(!bytes::validate_linked(trace, registry, []).valid);
    //! assert!(!bytes::validate_bundle(
    //!     Path::new("capture"),
    //!     BundleBytes::new(manifest, registry, trace),
    //! ).valid);
    //! ```

    use super::{BundleBytes, DocumentBytes, Path, ValidationLevel, ValidationReport, bundle, check_linked, load_bytes, registry};

    /// Validate supplied root-registry and import bytes without filesystem access.
    #[must_use]
    pub fn validate_registry<'a>(root: DocumentBytes<'a>, imports: impl AsRef<[DocumentBytes<'a>]>) -> ValidationReport {
        ValidationReport::new(
            ValidationLevel::Registry,
            root.path,
            registry::load_bytes(root, imports.as_ref()).issues,
        )
    }

    /// Validate supplied OTLP JSON or JSONL bytes at Trace Core level.
    #[must_use]
    pub fn validate_trace(document: DocumentBytes<'_>) -> ValidationReport {
        ValidationReport::new(
            ValidationLevel::Trace,
            document.path,
            load_bytes(document.path, document.bytes).issues,
        )
    }

    /// Validate supplied trace and registry bytes at Linked level.
    #[must_use]
    pub fn validate_linked<'a>(
        trace: DocumentBytes<'a>,
        root: DocumentBytes<'a>,
        imports: impl AsRef<[DocumentBytes<'a>]>,
    ) -> ValidationReport {
        let registry = registry::load_bytes(root, imports.as_ref());
        let parsed = load_bytes(trace.path, trace.bytes);
        let mut issues = registry.issues;
        issues.extend(parsed.issues);
        if issues.is_empty()
            && let (Some(registry), Some(parsed)) = (registry.value.as_ref(), parsed.value.as_ref())
        {
            check_linked(parsed, registry, &mut issues);
        }
        ValidationReport::new(ValidationLevel::Linked, trace.path, issues)
    }

    /// Validate the three named in-memory files in a capture bundle.
    #[must_use]
    pub fn validate_bundle(directory: impl AsRef<Path>, documents: BundleBytes<'_>) -> ValidationReport {
        let directory = directory.as_ref();
        ValidationReport::new(ValidationLevel::Bundle, directory, bundle::validate_bytes(directory, documents))
    }
}

pub(crate) fn issue(location: impl Into<String>, message: impl Into<String>, issues: &mut Vec<ValidationIssue>) {
    issues.push(ValidationIssue {
        location: location.into(),
        message: message.into(),
    });
}
pub(crate) fn located(path: impl AsRef<Path>, location: impl AsRef<str>) -> String {
    format!("{}:{}", path.as_ref().display(), location.as_ref())
}
pub(crate) fn sha256_digest(bytes: impl AsRef<[u8]>) -> String {
    use sha2::{Digest as _, Sha256};
    format!("sha256:{:x}", Sha256::digest(bytes.as_ref()))
}
pub(crate) fn is_digest(value: impl AsRef<str>) -> bool {
    // SHA-256 is 32 bytes, rendered as two lowercase hexadecimal digits per byte.
    // Changing this length would accept or reject non-SHA-256 registry digests.
    const SHA256_HEX_LEN: usize = 64;
    let value = value.as_ref();
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == SHA256_HEX_LEN && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
}
pub(crate) fn finish_read<E: fmt::Display>(
    path: impl AsRef<Path>,
    result: Result<Vec<u8>, E>,
    issues: &mut Vec<ValidationIssue>,
) -> Option<Vec<u8>> {
    let path = path.as_ref();
    result
        .map_err(|error| issue(located(path, "$"), format!("failed to read file: {error}"), issues))
        .ok()
}

#[cfg(feature = "test-util")]
pub mod test_util {
    //! Injected controls that exercise actual path-validator behavior without filesystem access.

    use super::ValidationReport;
    use crate::comparison::{DocumentReader, LoadError};
    use std::path::{Path, PathBuf};

    struct FakeReader {
        result: Result<Vec<u8>, std::io::ErrorKind>,
    }
    impl DocumentReader for FakeReader {
        fn read(&self, path: &Path) -> Result<Vec<u8>, LoadError> {
            self.result
                .clone()
                .map_err(std::io::Error::from)
                .map_err(|error| LoadError::reading(path, error))
        }
        fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError> {
            Ok(path.to_path_buf())
        }
    }

    /// Validate a trace through the actual path validator with an injected read result.
    #[must_use]
    pub fn validate_trace(result: Result<Vec<u8>, std::io::ErrorKind>) -> ValidationReport {
        super::reader::validate_trace(Path::new("injected.otlp.json"), &FakeReader { result })
    }
}

#[cfg(test)]
mod tests {
    use super::{ValidationIssue, finish_read};
    use std::path::Path;

    #[test]
    fn read_result_becomes_a_validation_issue() {
        let path = Path::new("artifact.json");
        let mut issues = Vec::new();
        let result: std::io::Result<Vec<u8>> = Err(std::io::ErrorKind::PermissionDenied.into());
        assert_eq!(finish_read(path, result, &mut issues), None);
        assert_eq!(
            issues,
            vec![ValidationIssue {
                location: "artifact.json:$".into(),
                message: "failed to read file: permission denied".into()
            }]
        );
    }
}
