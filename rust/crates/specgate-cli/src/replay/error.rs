//! Opaque replay failures and their CTSC protocol stage.
/// Machine-readable stage recorded for a failed replay operation.
///
/// This is CTSC protocol data, not the internal classification of [`Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "ReplayErrorKind")]
#[non_exhaustive]
pub(crate) enum FailureStage {
    /// The request was invalid.
    Request,
    /// The capture bundle could not be read or decoded.
    Capture,
    /// Candidate discovery or semantic linking failed.
    Linking,
    /// Runner execution failed.
    Execution,
    /// Candidate trace publication failed.
    Publication,
}
use specgate::SpecEvent;

/// Stable source spelling consumed by CTSC field metadata.
type ReplayErrorKind = FailureStage;

/// An opaque failure from [`super::replay`].
///
/// `Display` renders the CLI diagnostic. [`std::error::Error::source`] retains
/// an upstream cause when present, and [`ohno::ErrorExt::backtrace`] exposes the
/// captured backtrace separately.
///
/// Wrapped filesystem, process, and discovery causes remain in the standard
/// error chain. The CTSC projection records [`FailureStage`] independently from
/// the Rust error API.
///
/// # Examples
/// ```
/// use specgate_cli::replay::{Paths, Request};
/// let error = Request::builder(Paths::new("", "binding.yaml", "out.json"))
///     .build().unwrap_err();
/// assert!(!error.to_string().is_empty());
/// ```
// Workspace error policy requires `ohno`; this expansion supplies the documented
// source chain, backtrace, and `new`/`caused_by` constructors.
#[ohno::error]
#[display("{diagnostic}")]
#[derive(SpecEvent)]
#[spec_event(name = "ReplayError")]
pub struct Error {
    #[spec_event]
    kind: ReplayErrorKind,
    // `message` is the stable CTSC field name; changing it breaks replay metadata compatibility.
    #[spec_event(name = "message")]
    diagnostic: String,
}
impl Error {
    pub(crate) fn new_message(kind: FailureStage, value: impl Into<String>) -> Self {
        Self::new(kind, value.into())
    }
    pub(crate) fn wrap(kind: FailureStage, diagnostic: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(kind, diagnostic.into(), source)
    }

    pub(super) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

// Free-form strings originate in semantic linking helpers. Keeping this
// compatibility conversion in the Linking category preserves established CLI
// classification; changing the default would alter stage predicates and CTSC
// error metadata, so other stages use `new_message` or `wrap` explicitly.
impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::new_message(FailureStage::Linking, diagnostic)
    }
}
impl From<&str> for Error {
    fn from(diagnostic: &str) -> Self {
        Self::new_message(FailureStage::Linking, diagnostic)
    }
}

pub(super) fn failure<T>(diagnostic: impl Into<String>) -> Result<T, Error> {
    Err(Error::from(diagnostic.into()))
}

/// Stable source spelling consumed by operation metadata.
pub(super) type ReplayError = Error;
