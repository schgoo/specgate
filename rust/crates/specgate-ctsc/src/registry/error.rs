//! Opaque registry failures.

/// Registry failure with source-chain and backtrace support.
///
/// # Examples
/// ```
/// # let error: specgate_ctsc::registry::Error = "invalid input".to_string().into();
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
        Self::caused_by(source.to_string(), source)
    }
}
impl Error {
    pub(super) fn json(source: serde_json::Error) -> Self {
        Self::caused_by("registry JSON processing failed".to_string(), source)
    }
}
