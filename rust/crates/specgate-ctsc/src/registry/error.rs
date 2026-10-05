//! Structured registry failures.

/// Stable registry failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// A normalized schema was malformed or semantically inconsistent.
    Encoding,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("encoding")
    }
}

/// Structured registry failure with source-chain and backtrace support.
///
/// Errors retain a stable public classification even when diagnostics evolve.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::registry::error::Error = "invalid input".to_string().into();
/// assert_eq!(error.kind(), specgate_ctsc::registry::error::ErrorKind::Encoding);
/// ```
#[ohno::error]
#[display("{diagnostic}")]
pub struct Error {
    kind: ErrorKind,
    diagnostic: String,
}

impl Error {
    pub(super) fn message(diagnostic: String) -> Self {
        Self::new(ErrorKind::Encoding, diagnostic)
    }

    /// Whether this is a encoding failure.
    #[must_use]
    pub const fn is_encoding(&self) -> bool {
        matches!(self.kind, ErrorKind::Encoding)
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

/// Registry operation result.
pub type Result<T> = std::result::Result<T, Error>;

impl From<super::normalized::error::Error> for Error {
    fn from(source: super::normalized::error::Error) -> Self {
        Self::caused_by(ErrorKind::Encoding, source.to_string(), source)
    }
}
impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Self::caused_by(ErrorKind::Encoding, "registry JSON processing failed".to_string(), source)
    }
}
