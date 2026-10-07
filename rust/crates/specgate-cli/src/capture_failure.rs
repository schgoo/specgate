//! Opaque Rust error and private CTSC stage for the capture boundary.
//!
//! Library callers receive [`CaptureError`] and report it through [`Display`]
//! or inspect its standard source chain. The error intentionally exposes no
//! category, predicate, diagnostic, or path accessors.
//!
//! Annotated capture operations still require a machine-readable failure stage.
//! [`FailureStage`] supplies that private protocol value while preserving the
//! established CTSC wire name `CaptureErrorKind`; it is not part of the public
//! Rust error API. Dedicated crate-owned formatters retain the stable CLI line
//! protocol without exposing the error's internal fields.
//!
//! [`Display`]: std::fmt::Display
use specgate::SpecEvent;

/// Machine-readable stage recorded for a failed capture operation.
///
/// This is CTSC protocol data, not the internal classification of
/// [`CaptureError`]. The Rust error remains opaque to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[non_exhaustive]
#[spec_event(name = "CaptureErrorKind")]
pub(crate) enum FailureStage {
    /// The capture request could not be interpreted.
    Request,
    /// Binding resolution or target discovery failed.
    Discovery,
    /// Component selection failed.
    Selection,
    /// Building, enumerating, or running tests failed.
    Execution,
    /// Bundle encoding or artifact writing failed.
    Encoding,
}

// Preserve the established source spelling consumed by CTSC field metadata.
type CaptureErrorKind = FailureStage;

/// Actionable opaque capture failure.
///
/// `Display` renders the boundary diagnostic followed by an upstream cause and
/// captured backtrace when present. [`std::error::Error::source`] and
/// [`ohno::ErrorExt::backtrace`] expose those details programmatically.
///
/// # Examples
///
/// ```
/// use specgate_cli::{CapturePaths, CaptureRequest};
///
/// let error = CaptureRequest::builder(CapturePaths {
///     binding: "binding.yaml".into(),
///     out: "".into(),
/// })
/// .build()
/// .unwrap_err();
/// assert_eq!(error.to_string(), "capture requires a non-empty output directory");
/// ```
// Workspace error policy requires `ohno`; this expansion supplies the documented
// source chain, backtrace, and `new`/`caused_by` constructors.
#[ohno::error]
#[display("{diagnostic}")]
#[derive(SpecEvent)]
#[spec_event(name = "CaptureError")]
pub struct CaptureError {
    #[spec_event]
    kind: CaptureErrorKind,
    #[spec_event(name = "message")]
    diagnostic: String,
}

impl CaptureError {
    pub(crate) fn message(kind: CaptureErrorKind, diagnostic: impl AsRef<str>) -> Self {
        Self::new(kind, diagnostic.as_ref().to_owned())
    }

    pub(crate) fn wrap(
        kind: CaptureErrorKind,
        diagnostic: impl AsRef<str>,
        source: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    ) -> Self {
        Self::caused_by(kind, diagnostic.as_ref().to_owned(), source)
    }

    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }

    #[cfg(test)]
    pub(crate) const fn stage(&self) -> FailureStage {
        self.kind
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureError, CaptureErrorKind};

    #[test]
    fn error_contract() {
        use std::error::Error as _;

        let plain = CaptureError::message(CaptureErrorKind::Selection, "select a component");
        assert_eq!(plain.to_string(), "select a component");
        assert!(plain.source().is_none());
        let _status = ohno::ErrorExt::backtrace(&plain);

        let caused = CaptureError::wrap(
            CaptureErrorKind::Encoding,
            "write bundle",
            std::io::Error::other("disk unavailable"),
        );
        assert_eq!(caused.to_string(), "write bundle\ncaused by: disk unavailable");
        assert_eq!(caused.source().map(ToString::to_string).as_deref(), Some("disk unavailable"));
    }
}
