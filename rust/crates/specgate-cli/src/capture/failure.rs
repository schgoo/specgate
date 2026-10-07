//! Capture-stage diagnostics and conversion into the stable public error taxonomy.
//!
//! [`FailureContext`] preserves low-level sources until the capture facade
//! assigns a stable CTSC capture stage and publishes an opaque [`CaptureError`].
use super::{CaptureError, CaptureErrorKind};

/// Internal diagnostic value that preserves an optional low-level source.
///
/// Capture stages use this type until the facade assigns the stable public
/// protocol stage corresponding to the failed workflow stage.
#[derive(Debug)]
pub(crate) struct FailureContext {
    diagnostic: String,
    source: Option<ohno::AppError>,
}

impl FailureContext {
    pub(super) fn domain(diagnostic: impl AsRef<str>) -> Self {
        Self {
            diagnostic: diagnostic.as_ref().to_owned(),
            source: None,
        }
    }

    pub(super) fn with_source(diagnostic: impl AsRef<str>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self {
            diagnostic: diagnostic.as_ref().to_owned(),
            source: Some(ohno::AppError::new(source)),
        }
    }

    #[cfg(test)]
    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

impl From<specgate_discovery::Error> for FailureContext {
    fn from(error: specgate_discovery::Error) -> Self {
        let diagnostic = error.to_string();
        Self::with_source(diagnostic, error)
    }
}

impl From<String> for FailureContext {
    fn from(diagnostic: String) -> Self {
        Self::domain(diagnostic)
    }
}

impl From<&str> for FailureContext {
    fn from(diagnostic: &str) -> Self {
        Self::domain(diagnostic)
    }
}

#[cfg(any(test, feature = "test-util"))]
impl PartialEq<&str> for FailureContext {
    fn eq(&self, other: &&str) -> bool {
        self.diagnostic == *other
    }
}

pub(super) fn public_error(kind: CaptureErrorKind) -> impl FnOnce(FailureContext) -> CaptureError {
    move |error| {
        if let Some(source) = error.source {
            CaptureError::wrap(kind, error.diagnostic, source)
        } else {
            CaptureError::message(kind, error.diagnostic)
        }
    }
}
