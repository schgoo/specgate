//! Structured normalized-schema conversion diagnostics.
#[ohno::error]
#[display("{diagnostic}")]
pub(crate) struct Error {
    diagnostic: String,
}
impl Error {
    pub(crate) fn malformed(source: serde_json::Error) -> Self {
        Self::caused_by("malformed normalized schema JSON".to_string(), source)
    }
}

impl From<serde_json::Error> for Error {
    fn from(source: serde_json::Error) -> Self {
        Self::malformed(source)
    }
}
impl From<String> for Error {
    fn from(diagnostic: String) -> Self {
        Self::new(diagnostic)
    }
}
pub(crate) type Result<T> = std::result::Result<T, Error>;
