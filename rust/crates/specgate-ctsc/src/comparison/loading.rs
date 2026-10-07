//! Document-loading boundary used by path-oriented comparison.

#[cfg(any(test, feature = "test-util"))]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Operation {
    Read,
    Canonicalize,
}
impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Read => "read",
            Self::Canonicalize => "canonicalize",
        })
    }
}

/// Crate-owned failure to load or resolve a comparison document.
#[ohno::error]
#[display("failed to {operation} document '{}'", path.display())]
pub struct LoadError {
    operation: Operation,
    path: PathBuf,
}
impl LoadError {
    /// Wrap a document read failure with its path, source, and a captured backtrace.
    #[must_use]
    pub fn reading(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::caused_by(Operation::Read, path.as_ref().to_path_buf(), source)
    }
    /// Wrap a path-resolution failure with its path, source, and a captured backtrace.
    #[must_use]
    pub fn canonicalizing(path: impl AsRef<Path>, source: std::io::Error) -> Self {
        Self::caused_by(Operation::Canonicalize, path.as_ref().to_path_buf(), source)
    }
}

/// User-extensible document loading boundary for path-based comparison and validation.
///
/// # Examples
/// ```
/// use specgate_ctsc::comparison::{DocumentReader, LoadError};
/// use std::path::{Path, PathBuf};
/// struct Memory;
/// impl DocumentReader for Memory {
///     fn read(&self, _path: &Path) -> Result<Vec<u8>, LoadError> { Ok(b"{}".to_vec()) }
///     fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError> { Ok(path.to_path_buf()) }
/// }
/// let bytes = Memory.read(Path::new("trace.json"))?;
/// assert!(serde_json::from_slice::<serde_json::Value>(&bytes).is_ok());
/// # Ok::<(), LoadError>(())
/// ```
pub trait DocumentReader: Send + Sync {
    /// Read one comparison document.
    ///
    /// # Errors
    /// Returns a contextual document-loading failure.
    fn read(&self, path: &Path) -> Result<Vec<u8>, LoadError>;
    /// Resolve one document path for import identity.
    ///
    /// # Errors
    /// Returns a contextual path-resolution failure.
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError>;
}

/// Real operating-system document reader used by path-oriented APIs.
///
/// # Examples
/// ```no_run
/// use specgate_ctsc::comparison::SystemReader;
/// let bytes = SystemReader::system().read("trace.otlp.json")?;
/// assert!(!bytes.is_empty());
/// # Ok::<(), specgate_ctsc::comparison::LoadError>(())
/// ```
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct SystemReader {
    inner: Arc<ReaderKind>,
}
#[derive(Debug)]
enum ReaderKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(BTreeMap<PathBuf, Vec<u8>>),
}
impl SystemReader {
    /// Create a reader backed by the operating-system filesystem.
    #[must_use]
    pub fn system() -> Self {
        Self {
            inner: Arc::new(ReaderKind::Real),
        }
    }

    /// Create a deterministic in-memory reader for tests and tooling.
    #[cfg(any(test, feature = "test-util"))]
    pub fn fake(files: impl IntoIterator<Item = (PathBuf, Vec<u8>)>) -> Self {
        Self {
            inner: Arc::new(ReaderKind::Fake(files.into_iter().collect())),
        }
    }

    /// Read one document from the operating-system filesystem.
    ///
    /// # Errors
    /// Returns a contextual filesystem read error.
    pub fn read(&self, path: impl AsRef<Path>) -> Result<Vec<u8>, LoadError> {
        let path = path.as_ref();
        match self.inner.as_ref() {
            ReaderKind::Real => std::fs::read(path).map_err(|error| LoadError::reading(path, error)),
            #[cfg(any(test, feature = "test-util"))]
            ReaderKind::Fake(files) => files
                .get(path)
                .cloned()
                .ok_or_else(|| LoadError::reading(path, std::io::Error::new(std::io::ErrorKind::NotFound, "fake document is absent"))),
        }
    }
    /// Resolve one document to its canonical filesystem path.
    ///
    /// # Errors
    /// Returns a contextual filesystem path-resolution error.
    pub fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, LoadError> {
        let path = path.as_ref();
        match self.inner.as_ref() {
            ReaderKind::Real => std::fs::canonicalize(path).map_err(|error| LoadError::canonicalizing(path, error)),
            #[cfg(any(test, feature = "test-util"))]
            ReaderKind::Fake(files) => files.contains_key(path).then(|| path.to_path_buf()).ok_or_else(|| {
                LoadError::canonicalizing(path, std::io::Error::new(std::io::ErrorKind::NotFound, "fake document is absent"))
            }),
        }
    }
}
impl DocumentReader for SystemReader {
    fn read(&self, path: &Path) -> Result<Vec<u8>, LoadError> {
        self.read(path)
    }
    fn canonicalize(&self, path: &Path) -> Result<PathBuf, LoadError> {
        self.canonicalize(path)
    }
}
