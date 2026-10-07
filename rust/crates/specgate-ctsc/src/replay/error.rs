//! Opaque replay failures.

/// Replay failure with source-chain and backtrace support.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::replay::Error = "invalid input".to_string().into();
/// assert_eq!(error.to_string(), "invalid input");
/// ```
#[ohno::error]
#[display("{diagnostic}")]
pub struct Error {
    diagnostic: String,
}

impl Error {
    pub(super) fn message(diagnostic: String) -> Self {
        Self::new(diagnostic)
    }
    pub(super) fn json(diagnostic: impl Into<String>, source: serde_json::Error) -> Self {
        Self::caused_by(diagnostic.into(), source)
    }
}

impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::message(diagnostic)
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(source: std::num::TryFromIntError) -> Self {
        Self::caused_by("integer value is outside its declared replay type", source)
    }
}
