//! Filesystem access and invocation-unique temporary directories.
//!
//! `Filesystem` selects a concrete real or deterministic fake backend. Real
//! operations translate I/O failures into crate-owned errors. Tests configure
//! `FakeFs` with files, directory listings, keyed failures, and unique
//! directory outcomes, then inspect shared fake state. Unique directories own
//! their paths and remove them on drop; injected collisions cause allocation to
//! retry, while injected failures are returned as operational errors.
//!
//! Test modules construct a deterministic adapter without touching the host:
//!
//! ```ignore
//! let state = FakeFs::builder().files(files).build();
//! let filesystem = Filesystem::fake(state);
//! assert_eq!(filesystem.read_to_string("binding.yaml")?, "language: rust");
//! # Ok::<(), crate::Error>(())
//! ```

use super::failure;
#[cfg(any(test, feature = "test-util"))]
use crate::ErrorKind;
use crate::error::Error;
#[cfg(any(test, feature = "test-util"))]
use std::cell::RefCell;
#[cfg(any(test, feature = "test-util"))]
use std::collections::{BTreeMap, VecDeque};
#[cfg(any(test, feature = "test-util"))]
use std::ffi::OsString;
use std::path::{Path, PathBuf};
#[cfg(any(test, feature = "test-util"))]
use std::rc::Rc;

/// Concrete real/fake filesystem adapter used by discovery runners.
#[derive(Clone)]
pub(crate) struct Filesystem {
    inner: FilesystemKind,
}

#[derive(Clone)]
enum FilesystemKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Rc<RefCell<FakeFs>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Operation {
    Canonicalize,
    CreateDirAll,
    ReadDir,
    ReadToString,
    Write,
    RemoveDirAll,
    CreateUniqueDir,
}

#[cfg(any(test, feature = "test-util"))]
impl Operation {
    /// Parse a stable fake-operation spelling; unknown operations return `None`.
    pub(crate) fn parse(value: impl AsRef<str>) -> Option<Self> {
        let value = value.as_ref();
        Some(match value {
            "canonicalize" => Self::Canonicalize,
            "create_dir_all" => Self::CreateDirAll,
            "read_dir" => Self::ReadDir,
            "read_to_string" => Self::ReadToString,
            "write" => Self::Write,
            "remove_dir_all" => Self::RemoveDirAll,
            "create_unique_dir" => Self::CreateUniqueDir,
            _ => return None,
        })
    }
}

#[cfg(any(test, feature = "test-util"))]
/// Parsed operation/path key used for deterministic failure injection.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FailureKey {
    operation: Operation,
    file: Option<OsString>,
}

#[cfg(any(test, feature = "test-util"))]
impl FailureKey {
    /// Parse a stable fake-operation spelling; unknown operations return `None`.
    pub(crate) fn parse(value: impl AsRef<str>) -> Option<Self> {
        let value = value.as_ref();
        let (operation, file) = value
            .split_once(':')
            .map_or((value, None), |(operation, file)| (operation, Some(OsString::from(file))));
        Some(Self {
            operation: Operation::parse(operation)?,
            file,
        })
    }

    #[cfg(test)]
    /// Parse a test fixture key, panicking with the rejected spelling when invalid.
    pub(crate) fn test(value: impl AsRef<str>) -> Self {
        let value = value.as_ref();
        Self::parse(value).unwrap_or_else(|| panic!("unknown fake filesystem failure key: {value}"))
    }

    fn new(operation: Operation, path: Option<&Path>) -> Self {
        Self {
            operation,
            file: path.and_then(Path::file_name).map(OsString::from),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// One deterministic unique-directory allocation outcome.
pub(crate) enum UniqueCandidate {
    Path(PathBuf),
    Collision,
    Failure(String),
}

#[cfg(any(test, feature = "test-util"))]
/// Reserved fake result spelling that requests a deterministic path collision.
const COLLISION_RESULT: &str = "collision";

#[cfg(any(test, feature = "test-util"))]
impl From<Result<PathBuf, String>> for UniqueCandidate {
    fn from(result: Result<PathBuf, String>) -> Self {
        match result {
            Ok(path) => Self::Path(path),
            Err(error) if error == COLLISION_RESULT => Self::Collision,
            Err(error) => Self::Failure(error),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// In-memory filesystem state, failure queues, and lifecycle observations.
#[derive(Default)]
pub(crate) struct FakeFs {
    files: BTreeMap<PathBuf, Vec<u8>>,
    directories: BTreeMap<PathBuf, Vec<PathBuf>>,
    directory_results: BTreeMap<PathBuf, Vec<Result<PathBuf, String>>>,
    canonical: BTreeMap<PathBuf, Result<PathBuf, String>>,
    failures: BTreeMap<FailureKey, VecDeque<String>>,
    unique_candidates: VecDeque<UniqueCandidate>,
    removed: Vec<PathBuf>,
}

#[cfg(any(test, feature = "test-util"))]
/// Builder for deterministic fake filesystem state.
#[derive(Default)]
pub(crate) struct FakeFsBuilder(FakeFs);

#[cfg(any(test, feature = "test-util"))]
impl FakeFs {
    /// Begin empty deterministic fake filesystem state.
    pub(crate) fn builder() -> FakeFsBuilder {
        FakeFsBuilder::default()
    }

    #[cfg(test)]
    /// Remove one seeded fake file.
    pub(crate) fn remove_file(&mut self, path: impl AsRef<Path>) {
        self.files.remove(path.as_ref());
    }

    #[cfg(test)]
    /// Insert or replace one fake file.
    pub(crate) fn insert_file(&mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        self.files.insert(path.into(), contents.into());
    }

    #[cfg(test)]
    /// Queue an operational failure for an exact or operation-wide key.
    pub(crate) fn fail(&mut self, key: FailureKey, message: impl Into<String>) {
        self.failures.entry(key).or_default().push_back(message.into());
    }

    #[cfg(test)]
    /// Whether fake state contains a file at `path`.
    pub(crate) fn has_file(&self, path: impl AsRef<Path>) -> bool {
        self.files.contains_key(path.as_ref())
    }

    #[cfg(test)]
    /// Whether fake state contains a directory at `path`.
    pub(crate) fn has_directory(&self, path: impl AsRef<Path>) -> bool {
        self.directories.contains_key(path.as_ref())
    }

    #[cfg(test)]
    /// Directories whose owning guards requested removal, in call order.
    pub(crate) fn removed(&self) -> &[PathBuf] {
        &self.removed
    }
}

#[cfg(any(test, feature = "test-util"))]
impl FakeFsBuilder {
    /// Seed complete file contents.
    pub(crate) fn files(mut self, value: BTreeMap<PathBuf, Vec<u8>>) -> Self {
        self.0.files = value;
        self
    }
    #[cfg(test)]
    /// Seed deterministic directory entries.
    pub(crate) fn directories(mut self, value: BTreeMap<PathBuf, Vec<PathBuf>>) -> Self {
        self.0.directories = value;
        self
    }
    #[cfg(test)]
    /// Seed per-entry directory successes and failures.
    pub(crate) fn directory_results(mut self, value: BTreeMap<PathBuf, Vec<Result<PathBuf, String>>>) -> Self {
        self.0.directory_results = value;
        self
    }
    /// Seed canonical path results; unseeded paths round-trip unchanged.
    pub(crate) fn canonical(mut self, value: BTreeMap<PathBuf, Result<PathBuf, String>>) -> Self {
        self.0.canonical = value;
        self
    }
    /// Seed queued operation failures consumed in order.
    pub(crate) fn failures(mut self, value: BTreeMap<FailureKey, VecDeque<String>>) -> Self {
        self.0.failures = value;
        self
    }
    #[cfg(test)]
    /// Seed unique-directory paths, collisions, and failures.
    pub(crate) fn unique_candidates(mut self, value: VecDeque<Result<PathBuf, String>>) -> Self {
        self.0.unique_candidates = value.into_iter().map(UniqueCandidate::from).collect();
        self
    }
    /// Finish deterministic fake state.
    pub(crate) fn build(self) -> FakeFs {
        self.0
    }
}

pub(crate) enum UniqueDirectory {
    Real(tempfile::TempDir),
    #[cfg(any(test, feature = "test-util"))]
    Fake {
        path: PathBuf,
        filesystem: Filesystem,
    },
}

impl UniqueDirectory {
    /// Borrow the owned unique directory path until the guard is dropped.
    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Real(directory) => directory.path(),
            #[cfg(any(test, feature = "test-util"))]
            Self::Fake { path, .. } => path,
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Drop for UniqueDirectory {
    fn drop(&mut self) {
        #[cfg(any(test, feature = "test-util"))]
        if let Self::Fake { path, filesystem } = self {
            let _ = filesystem.remove_dir_all(path);
        }
    }
}

impl Filesystem {
    /// Select host filesystem operations with crate-owned error conversion.
    pub(super) fn real() -> Self {
        Self {
            inner: FilesystemKind::Real,
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    /// Select isolated in-memory state; clones observe the same state.
    pub(crate) fn fake(state: FakeFs) -> Self {
        Self {
            inner: FilesystemKind::Fake(Rc::new(RefCell::new(state))),
        }
    }

    #[cfg(test)]
    /// Share fake state for assertions; panics when called on the real backend.
    pub(crate) fn fake_state(&self) -> Rc<RefCell<FakeFs>> {
        match &self.inner {
            FilesystemKind::Fake(state) => Rc::clone(state),
            FilesystemKind::Real => panic!("real filesystem has no fake state"),
        }
    }

    #[cfg(any(test, feature = "test-util"))]
    fn failure(&self, operation: Operation, path: Option<&Path>) -> Option<String> {
        if let FilesystemKind::Fake(state) = &self.inner {
            let mut state = state.borrow_mut();
            let exact = FailureKey::new(operation, path);
            if exact.file.is_some()
                && let Some(error) = state.failures.get_mut(&exact).and_then(VecDeque::pop_front)
            {
                return Some(error);
            }
            let general = FailureKey { operation, file: None };
            return state.failures.get_mut(&general).and_then(VecDeque::pop_front);
        }
        None
    }
    #[cfg(not(any(test, feature = "test-util")))]
    fn failure(&self, operation: Operation, path: Option<&Path>) -> Option<String> {
        let _ = (&self.inner, operation, path);
        None
    }

    /// Canonicalize a path, returning system failures as crate-owned errors.
    pub(crate) fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::Canonicalize, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => Ok(std::fs::canonicalize(path)?),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => state
                .borrow()
                .canonical
                .get(path)
                .cloned()
                .unwrap_or_else(|| Ok(path.to_path_buf()))
                .map_err(failure),
        }
    }

    /// Create a directory tree or record it in fake state.
    pub(crate) fn create_dir_all(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::CreateDirAll, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => {
                std::fs::create_dir_all(path)?;
                Ok(())
            }
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => {
                state.borrow_mut().directories.entry(path.to_path_buf()).or_default();
                Ok(())
            }
        }
    }

    /// Read directory entries while preserving per-entry failures.
    pub(crate) fn read_dir(&self, path: impl AsRef<Path>) -> Result<Vec<Result<PathBuf, Error>>, Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::ReadDir, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => Ok(std::fs::read_dir(path)?
                .map(|entry| -> Result<_, Error> { Ok(entry?.path()) })
                .collect()),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => {
                let state = state.borrow();
                Ok(state.directory_results.get(path).cloned().map_or_else(
                    || {
                        state
                            .directories
                            .get(path)
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .map(Ok)
                            .collect()
                    },
                    |entries| entries.into_iter().map(|entry| entry.map_err(failure)).collect(),
                ))
            }
        }
    }

    /// Read UTF-8 text, rejecting missing or malformed fake contents.
    pub(crate) fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::ReadToString, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => Ok(std::fs::read_to_string(path)?),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => state
                .borrow()
                .files
                .get(path)
                .ok_or_else(|| failure("file not found"))
                .and_then(|bytes| {
                    String::from_utf8(bytes.clone()).map_err(|source| Error::cause(ErrorKind::System, source.to_string(), source))
                }),
        }
    }

    /// Replace complete file bytes in the selected backend.
    pub(crate) fn write(&self, path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<(), Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::Write, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => {
                std::fs::write(path, contents)?;
                Ok(())
            }
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => {
                state.borrow_mut().files.insert(path.to_path_buf(), contents.as_ref().to_vec());
                Ok(())
            }
        }
    }

    /// Whether a file or directory exists without consuming failure queues.
    pub(crate) fn exists(&self, path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        match &self.inner {
            FilesystemKind::Real => path.exists(),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => state.borrow().files.contains_key(path) || state.borrow().directories.contains_key(path),
        }
    }

    /// Remove a directory tree; fake state records lifecycle cleanup.
    pub(crate) fn remove_dir_all(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        let path = path.as_ref();
        if let Some(error) = self.failure(Operation::RemoveDirAll, Some(path)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => {
                std::fs::remove_dir_all(path)?;
                Ok(())
            }
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => {
                state.borrow_mut().removed.push(path.to_path_buf());
                Ok(())
            }
        }
    }

    /// Allocate an invocation-unique owned directory below `parent`.
    ///
    /// Fake candidates are consumed until a path or operational failure occurs.
    pub(crate) fn unique_dir(&self, parent: impl AsRef<Path>, prefix: impl AsRef<str>) -> Result<UniqueDirectory, Error> {
        let parent = parent.as_ref();
        let prefix = prefix.as_ref();
        if let Some(error) = self.failure(Operation::CreateUniqueDir, Some(parent)) {
            return Err(failure(error));
        }
        match &self.inner {
            FilesystemKind::Real => {
                std::fs::create_dir_all(parent)?;
                Ok(UniqueDirectory::Real(tempfile::Builder::new().prefix(prefix).tempdir_in(parent)?))
            }
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(state) => {
                let path = loop {
                    match state
                        .borrow_mut()
                        .unique_candidates
                        .pop_front()
                        .unwrap_or_else(|| panic!("fake unique-directory queue exhausted; enqueue an allocation result"))
                    {
                        UniqueCandidate::Collision => {}
                        UniqueCandidate::Failure(error) => return Err(failure(error)),
                        UniqueCandidate::Path(path) => break path,
                    }
                };
                state.borrow_mut().directories.insert(path.clone(), Vec::new());
                Ok(UniqueDirectory::Fake {
                    path,
                    filesystem: self.clone(),
                })
            }
        }
    }
}
