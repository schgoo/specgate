//! Structured replay failures.

/// Stable replay failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Bundle bytes were malformed, inconsistent, or unsupported.
    Decoding,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("decoding")
    }
}

/// Structured replay failure with source-chain and backtrace support.
///
/// Errors retain a stable public classification even when diagnostics evolve.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::replay::Error = "invalid input".to_string().into();
/// assert_eq!(error.kind(), specgate_ctsc::replay::ErrorKind::Decoding);
/// ```
#[ohno::error]
#[display("{diagnostic}")]
pub struct Error {
    kind: ErrorKind,
    diagnostic: String,
}

impl Error {
    pub(super) fn message(diagnostic: String) -> Self {
        Self::new(ErrorKind::Decoding, diagnostic)
    }
    pub(super) fn json(diagnostic: impl Into<String>, source: serde_json::Error) -> Self {
        Self::caused_by(ErrorKind::Decoding, diagnostic.into(), source)
    }

    /// Whether this is a decoding failure.
    #[must_use]
    pub const fn is_decoding(&self) -> bool {
        matches!(self.kind, ErrorKind::Decoding)
    }

    /// Stable failure classification.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::message(diagnostic)
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(source: std::num::TryFromIntError) -> Self {
        Self::caused_by(ErrorKind::Decoding, "integer value is outside its declared replay type", source)
    }
}
