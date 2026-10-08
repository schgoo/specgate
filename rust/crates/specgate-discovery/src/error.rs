//! Opaque failures from discovery workflows.

/// Internal stage at which a discovery workflow failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ErrorKind {
    /// Binding loading, parsing, validation, or target selection failed.
    Binding,
    /// Raw registry JSON could not be parsed.
    Registry,
    /// Registry metadata could not be normalized.
    Normalization,
    /// Cargo metadata, runner generation, or runner execution failed.
    Cargo,
    /// C# project building or reflection failed.
    CSharp,
    /// An operating-system resource could not be accessed.
    System,
}

/// An actionable opaque discovery failure with a source chain.
///
/// Errors are obtained from discovery APIs rather than constructed by callers.
/// Display and the standard source chain preserve the situational detail.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::binding::resolve_target;
/// use std::error::Error as _;
///
/// let error = resolve_target("missing-binding.yaml", None).unwrap_err();
/// eprintln!("{error}");
/// if let Some(source) = error.source() {
///     eprintln!("caused by: {source}");
/// }
/// ```
#[ohno::error]
#[display("{diagnostic}")]
pub struct Error {
    kind: ErrorKind,
    diagnostic: String,
}

impl Error {
    #[cfg(test)]
    pub(crate) const fn is_registry(&self) -> bool {
        matches!(self.kind, ErrorKind::Registry)
    }
    #[cfg(test)]
    pub(crate) const fn is_normalization(&self) -> bool {
        matches!(self.kind, ErrorKind::Normalization)
    }
    #[cfg(test)]
    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }

    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(all(feature = "test-util", not(test)), expect(dead_code, reason = "feature-enabled test helper"))]
    pub(crate) fn contains(&self, pattern: impl AsRef<str>) -> bool {
        self.diagnostic.contains(pattern.as_ref())
    }
    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(all(feature = "test-util", not(test)), expect(dead_code, reason = "feature-enabled test helper"))]
    pub(crate) fn strip_prefix(&self, prefix: impl AsRef<str>) -> Option<&str> {
        self.diagnostic.strip_prefix(prefix.as_ref())
    }

    pub(crate) fn message(kind: ErrorKind, diagnostic: impl Into<String>) -> Self {
        Self::new(kind, diagnostic.into())
    }
    pub(crate) fn cause(kind: ErrorKind, diagnostic: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(kind, diagnostic.into(), source)
    }
}

impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Self::cause(ErrorKind::System, source.to_string(), source)
    }
}

#[cfg(test)]
impl PartialEq<&str> for Error {
    fn eq(&self, other: &&str) -> bool {
        self.diagnostic == *other
    }
}
