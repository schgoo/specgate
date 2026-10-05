//! Structured failures from discovery workflows.

/// Stable stage at which a discovery workflow failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
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

/// An actionable discovery failure with a stable stage and source chain.
///
/// Errors are obtained from discovery APIs rather than constructed by callers.
/// Classification, user-facing diagnostics, and the standard source chain can
/// then be inspected independently.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::binding::resolve_target;
/// use std::error::Error as _;
///
/// let error = resolve_target("missing-binding.yaml", None).unwrap_err();
/// assert!(error.is_binding());
/// eprintln!("{}", error.diagnostic());
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
    /// Whether binding loading or selection failed.
    #[must_use]
    pub const fn is_binding(&self) -> bool {
        matches!(self.kind, ErrorKind::Binding)
    }
    /// Whether registry parsing failed.
    #[must_use]
    pub const fn is_registry(&self) -> bool {
        matches!(self.kind, ErrorKind::Registry)
    }
    /// Whether semantic normalization failed.
    #[must_use]
    pub const fn is_normalization(&self) -> bool {
        matches!(self.kind, ErrorKind::Normalization)
    }
    /// Whether Cargo discovery failed.
    #[must_use]
    pub const fn is_cargo(&self) -> bool {
        matches!(self.kind, ErrorKind::Cargo)
    }
    /// Whether C# discovery failed.
    #[must_use]
    pub const fn is_csharp(&self) -> bool {
        matches!(self.kind, ErrorKind::CSharp)
    }
    /// Whether an operating-system interaction failed.
    #[must_use]
    pub const fn is_system(&self) -> bool {
        matches!(self.kind, ErrorKind::System)
    }
    /// Return the exact actionable text intended for users.
    #[must_use]
    pub fn diagnostic(&self) -> &str {
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
