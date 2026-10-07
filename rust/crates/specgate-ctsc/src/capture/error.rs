//! Opaque capture failures.

/// Capture encoding failure with source-chain and backtrace support.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::capture::Error = "invalid capture".to_string().into();
/// assert_eq!(error.to_string(), "invalid capture");
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
    pub(super) fn conversion(diagnostic: impl Into<String>, source: std::num::TryFromIntError) -> Self {
        Self::caused_by(diagnostic.into(), source)
    }
}

impl Error {
    pub(super) fn json(source: serde_json::Error) -> Self {
        Self::caused_by("native CTSC OTLP serialization failed".to_string(), source)
    }
}

impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::message(diagnostic)
    }
}
