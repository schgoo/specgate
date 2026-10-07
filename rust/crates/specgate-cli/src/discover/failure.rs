//! Opaque discover failures and their CTSC protocol stage.
/// Machine-readable stage recorded for a failed discover operation.
///
/// This is CTSC protocol data, not the internal classification of [`Error`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "DiscoverErrorKind")]
#[non_exhaustive]
pub(crate) enum FailureStage {
    /// `Request` construction failed.
    Request,
    /// Binding or implementation discovery failed.
    Discovery,
    /// Registry encoding failed.
    Encoding,
    /// Registry persistence failed.
    Publication,
}
use specgate::SpecEvent;

/// Stable source spelling required by the established CTSC projected type name.
type DiscoverErrorKind = FailureStage;

/// An opaque failure from [`super::discover`].
///
/// Standard [`std::error::Error`] chaining retains filesystem and discovery
/// causes. The CTSC projection records [`FailureStage`] independently from the
/// Rust error API.
///
/// # Examples
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use specgate_cli::discover::{ComponentName, Params, Request, RegistryId, RegistryVersion};
/// let error = Request::builder(Params {
///     binding: "".into(), out: "registry.json".into(),
///     component: ComponentName::parse("example.math")?, registry_id: RegistryId::parse("id")?,
///     registry_version: RegistryVersion::parse("1")?,
/// }).build().unwrap_err();
/// assert!(!error.to_string().is_empty());
/// # Ok(())
/// # }
/// ```
#[ohno::error]
#[display("{diagnostic}")]
#[derive(SpecEvent)]
#[spec_event(name = "DiscoverError")]
pub struct Error {
    #[spec_event]
    kind: DiscoverErrorKind,
    // `message` is the established CTSC projected field name; changing it
    // would break registry and trace compatibility despite the Rust name.
    #[spec_event(name = "message")]
    diagnostic: String,
}

impl From<crate::system::DiscoveryFailure> for Error {
    fn from(error: crate::system::DiscoveryFailure) -> Self {
        Self::discovery(error.to_string(), error)
    }
}

impl Error {
    pub(crate) fn request(message: impl Into<String>) -> Self {
        Self::new(FailureStage::Request, message.into())
    }
    pub(crate) fn discovery(message: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(FailureStage::Discovery, message.into(), source)
    }
    pub(crate) fn encoding(message: impl Into<String>) -> Self {
        Self::new(FailureStage::Encoding, message.into())
    }
    pub(crate) fn publication(message: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(FailureStage::Publication, message.into(), source)
    }
    pub(super) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
    #[cfg(test)]
    pub(crate) const fn stage(&self) -> FailureStage {
        self.kind
    }
}

/// Stable source spelling consumed by operation metadata.
pub(super) type DiscoverError = Error;
