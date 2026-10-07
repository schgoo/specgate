#![cfg(any(test, feature = "test-util"))]

//! Structured failures produced while generating test-only golden artifacts.

/// Golden generation or trace-inspection failure.
#[ohno::error]
#[display("{diagnostic}")]
pub(super) struct GoldenError {
    diagnostic: String,
}
impl GoldenError {
    /// Construct a message-only generation failure without an underlying source.
    pub(super) fn message(value: impl Into<String>) -> Self {
        Self::new(value.into())
    }
    /// Wrap an operational source while retaining the golden-generation diagnostic.
    pub(super) fn wrap(diagnostic: impl Into<String>, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::caused_by(diagnostic.into(), source)
    }
}
impl From<String> for GoldenError {
    fn from(value: String) -> Self {
        Self::message(value)
    }
}

/// Compiler attribution details retained as source paths rather than strings.
#[derive(Debug)]
pub(super) struct Attribution {
    declared_sources: Vec<std::path::PathBuf>,
    unrelated_sources: Vec<std::path::PathBuf>,
    unattributed_count: usize,
}
impl Attribution {
    /// Classify compiler diagnostics by declared, unrelated, and unattributed sources.
    pub(super) fn new(
        declared_sources: impl IntoIterator<Item = std::path::PathBuf>,
        unrelated_sources: impl IntoIterator<Item = std::path::PathBuf>,
        unattributed_count: usize,
    ) -> Self {
        Self {
            declared_sources: declared_sources.into_iter().collect(),
            unrelated_sources: unrelated_sources.into_iter().collect(),
            unattributed_count,
        }
    }
}

/// Compiler rejection that escaped the matrix's declared intentional sources.
#[ohno::error]
#[display("{diagnostic}")]
pub(super) struct CompileError {
    attribution: Attribution,
    diagnostic: String,
}
impl CompileError {
    /// Construct a compiler failure with its structured source attribution.
    pub(super) fn attribution(diagnostic: impl Into<String>, attribution: Attribution) -> Self {
        let _ = (
            &attribution.declared_sources,
            &attribution.unrelated_sources,
            attribution.unattributed_count,
        );
        Self::new(attribution, diagnostic.into())
    }
}
