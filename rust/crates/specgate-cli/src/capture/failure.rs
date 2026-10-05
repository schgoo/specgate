//! Capture-stage diagnostics and conversion into the stable public error taxonomy.
//!
//! [`ContextError`] preserves low-level source chains until the capture facade
//! assigns a stable [`CaptureErrorKind`] and publishes [`CaptureError`].
use super::*;

/// Internal diagnostic context that preserves an optional low-level source.
///
/// Capture stages use this type until the facade assigns the stable public
/// [`CaptureErrorKind`] corresponding to the failed workflow stage.
#[ohno::error]
#[display("{diagnostic}")]
pub(crate) struct ContextError {
    diagnostic: String,
}

impl ContextError {
    pub(super) fn domain(diagnostic: impl AsRef<str>) -> Self {
        Self::new(diagnostic.as_ref().to_owned())
    }

    pub(super) fn with_source(diagnostic: impl AsRef<str>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(diagnostic.as_ref().to_owned(), source)
    }

    pub(super) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl From<specgate_discovery::Error> for ContextError {
    fn from(error: specgate_discovery::Error) -> Self {
        let diagnostic = error.to_string();
        Self::with_source(diagnostic, error)
    }
}

impl From<String> for ContextError {
    fn from(diagnostic: String) -> Self {
        Self::domain(diagnostic)
    }
}

impl From<&str> for ContextError {
    fn from(diagnostic: &str) -> Self {
        Self::domain(diagnostic)
    }
}

#[cfg(any(test, feature = "test-util"))]
impl PartialEq<&str> for ContextError {
    fn eq(&self, other: &&str) -> bool {
        self.diagnostic == *other
    }
}

pub(super) fn public_error(kind: CaptureErrorKind) -> impl FnOnce(ContextError) -> CaptureError {
    move |error| {
        let diagnostic = error.diagnostic().to_string();
        if error.source().is_some() {
            CaptureError::wrap(kind, diagnostic, error)
        } else {
            CaptureError::message(kind, diagnostic)
        }
    }
}
