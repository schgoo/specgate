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

/// An asynchronous schedule that sequential replay cannot reproduce.
///
/// This opaque cause is retained in the standard error chain so callers can
/// distinguish unsupported scheduling from invalid input without parsing text.
#[ohno::error]
#[display("{diagnostic}")]
pub struct UnsupportedScheduling {
    diagnostic: String,
}

impl Error {
    pub(super) fn message(diagnostic: String) -> Self {
        Self::new(diagnostic)
    }
    pub(super) fn json(diagnostic: impl Into<String>, source: serde_json::Error) -> Self {
        Self::caused_by(diagnostic.into(), source)
    }
    pub(super) fn scheduling(diagnostic: impl Into<String>) -> Self {
        Self::caused_by("unsupported asynchronous scheduling", UnsupportedScheduling::new(diagnostic.into()))
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

#[cfg(test)]
mod tests {
    #[test]
    fn scheduling_cause_does_not_depend_on_diagnostic_wording() {
        let error = super::Error::scheduling("independent diagnostic wording");
        assert!(std::error::Error::source(&error).unwrap().is::<super::UnsupportedScheduling>());
        assert_eq!(
            error.to_string(),
            "unsupported asynchronous scheduling\ncaused by: independent diagnostic wording"
        );
    }
}
