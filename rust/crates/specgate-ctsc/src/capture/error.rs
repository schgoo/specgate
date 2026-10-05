//! Structured capture failures.

/// Stable capture failure classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Capture evidence was structurally inconsistent or could not be encoded.
    Encoding,
}
impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("encoding")
    }
}
/// Structured capture encoding failure with source-chain and backtrace support.
///
/// Callers can use [`Error::is_encoding`] for stable classification while retaining
/// the full diagnostic and source chain.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::capture::error::Error = "invalid capture".to_string().into();
/// assert!(error.is_encoding());
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
    pub(super) fn conversion(diagnostic: impl Into<String>, source: std::num::TryFromIntError) -> Self {
        Self::caused_by(ErrorKind::Encoding, diagnostic.into(), source)
    }
    /// Whether this is an encoding failure.
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

impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Self::caused_by(ErrorKind::Encoding, "native CTSC OTLP serialization failed".to_string(), source)
    }
}

impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::message(diagnostic)
    }
}
