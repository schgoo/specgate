//! Canonical structured discover failures and their stable taxonomy.
/// Stable classification for a discovery failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "DiscoverErrorKind")]
#[non_exhaustive]
pub enum ErrorKind {
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
type DiscoverErrorKind = ErrorKind;

/// A structured failure from [`super::discover`].
///
/// The predicates classify the failed workflow stage without parsing the
/// human-readable diagnostic. Standard [`std::error::Error`] chaining retains
/// filesystem and discovery causes.
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
/// assert!(error.is_request());
/// assert!(!error.diagnostic().is_empty());
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
        Self::new(ErrorKind::Request, message.into())
    }
    pub(crate) fn discovery(message: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(ErrorKind::Discovery, message.into(), source)
    }
    pub(crate) fn encoding(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Encoding, message.into())
    }
    pub(crate) fn publication(message: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(ErrorKind::Publication, message.into(), source)
    }
    /// Whether request validation failed.
    #[must_use]
    pub const fn is_request(&self) -> bool {
        matches!(self.kind, ErrorKind::Request)
    }
    /// Whether target discovery failed.
    #[must_use]
    pub const fn is_discovery(&self) -> bool {
        matches!(self.kind, ErrorKind::Discovery)
    }
    /// Whether registry encoding failed.
    #[must_use]
    pub const fn is_encoding(&self) -> bool {
        matches!(self.kind, ErrorKind::Encoding)
    }
    /// Whether output publication failed.
    #[must_use]
    pub const fn is_publication(&self) -> bool {
        matches!(self.kind, ErrorKind::Publication)
    }
    /// Return the diagnostic rendered to CLI users.
    #[must_use]
    pub fn diagnostic(&self) -> &str {
        &self.diagnostic
    }
}

/// Stable source spelling consumed by operation metadata.
pub(super) type DiscoverError = Error;
