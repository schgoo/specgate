//! Canonical structured capture failure.
use crate::capture_error::CaptureErrorKind;
use specgate::SpecEvent;

/// Actionable capture failure with focused classification predicates.
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
/// assert!(error.is_request());
/// assert_eq!(error.diagnostic(), "capture requires a non-empty output directory");
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
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self::caused_by(kind, diagnostic.as_ref().to_owned(), source)
    }

    /// Whether request validation failed.
    #[must_use]
    pub const fn is_request(&self) -> bool {
        matches!(self.kind, CaptureErrorKind::Request)
    }

    /// Whether target discovery failed.
    #[must_use]
    pub const fn is_discovery(&self) -> bool {
        matches!(self.kind, CaptureErrorKind::Discovery)
    }

    /// Whether component selection failed.
    #[must_use]
    pub const fn is_selection(&self) -> bool {
        matches!(self.kind, CaptureErrorKind::Selection)
    }

    /// Whether test execution failed.
    #[must_use]
    pub const fn is_execution(&self) -> bool {
        matches!(self.kind, CaptureErrorKind::Execution)
    }

    /// Whether bundle encoding or writing failed.
    #[must_use]
    pub const fn is_encoding(&self) -> bool {
        matches!(self.kind, CaptureErrorKind::Encoding)
    }

    /// Return the boundary diagnostic displayed to CLI users.
    #[must_use]
    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::{CaptureError, CaptureErrorKind};

    #[test]
    fn error_contract() {
        use std::error::Error as _;

        let plain = CaptureError::message(CaptureErrorKind::Selection, "select a component");
        assert_eq!(plain.diagnostic(), "select a component");
        assert_eq!(plain.to_string(), "select a component");
        assert!(plain.source().is_none());
        let _status = ohno::ErrorExt::backtrace(&plain);

        let caused = CaptureError::wrap(
            CaptureErrorKind::Encoding,
            "write bundle",
            std::io::Error::other("disk unavailable"),
        );
        assert_eq!(caused.diagnostic(), "write bundle");
        assert_eq!(caused.source().map(ToString::to_string).as_deref(), Some("disk unavailable"));
    }
}
