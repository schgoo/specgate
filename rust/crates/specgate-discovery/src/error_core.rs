//! Private `ohno` source-chain storage for the public discovery error facade.

#[ohno::error]
#[display("{diagnostic}")]
pub(crate) struct ErrorCore {
    diagnostic: String,
}

impl ErrorCore {
    pub(crate) fn cause(diagnostic: impl AsRef<str>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(diagnostic.as_ref().to_owned(), source)
    }
}
