//! Atomic capture persistence behind concrete real and fake filesystem backends.

use super::{CaptureError, CaptureErrorKind};
use serde::Serialize;
use std::io::Write as _;
use std::path::Path;
#[cfg(any(test, feature = "test-util"))]
use std::sync::{Arc, Mutex};

/// How many times a sidecar replacement is retried before it is reported.
///
/// One scenario rewrites its sidecar after every top-level operation to preserve
/// crash recovery after each semantic completion. The same destination can
/// therefore be replaced many times. On Windows a replacement transiently
/// fails with "access is denied" whenever another process — a virus scanner or
/// search indexer — still holds the file it just
/// saw appear. Retrying is not papering over a race in the capture itself: the
/// serialized bytes are already complete and durable, and only the final rename
/// is retried. Twelve attempts with the linear delay below wait at most 660 ms,
/// enough to outlast short scanner locks without hiding a persistent failure.
const PERSIST_ATTEMPTS: u32 = 12;
// A 10 ms linear base yields delays from 10 through 110 ms before the final
// attempt, balancing scanner tolerance against capture-test latency.
const RETRY_MS: u64 = 10;

pub(super) fn persist_atomic(file_system: &FileSystem, path: impl AsRef<Path>, capture: &impl Serialize) -> Result<(), CaptureError> {
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    file_system.create_dir_all(parent).map_err(|error| {
        CaptureError::caused(
            CaptureErrorKind::Persistence,
            format!("failed to create native capture sidecar directory {}: {error}", parent.display()),
            error,
        )
    })?;
    let mut file = file_system.create_temp(parent).map_err(|error| {
        CaptureError::caused(
            CaptureErrorKind::Persistence,
            format!("failed to create native capture sidecar: {error}"),
            error,
        )
    })?;
    serde_json::to_writer(&mut file, capture).map_err(|error| {
        CaptureError::caused(
            CaptureErrorKind::Persistence,
            format!("failed to serialize native capture sidecar: {error}"),
            error,
        )
    })?;
    file.flush().map_err(|error| {
        CaptureError::caused(
            CaptureErrorKind::Persistence,
            format!("failed to flush native capture sidecar: {error}"),
            error,
        )
    })?;
    file.sync_all().map_err(|error| {
        CaptureError::caused(
            CaptureErrorKind::Persistence,
            format!("failed to sync native capture sidecar: {error}"),
            error,
        )
    })?;
    for attempt in 1..=PERSIST_ATTEMPTS {
        match file.replace(path) {
            Ok(()) => return Ok(()),
            Err(rejected) if attempt < PERSIST_ATTEMPTS => {
                file = rejected.file;
                file_system.wait(attempt);
            }
            Err(rejected) => {
                let diagnostic = format!(
                    "failed to atomically persist native capture sidecar {} after {PERSIST_ATTEMPTS} attempts: {}",
                    path.display(),
                    rejected.error
                );
                return Err(CaptureError::caused(CaptureErrorKind::Persistence, diagnostic, rejected.error));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct FileSystem {
    backend: FsBackend,
}

#[derive(Debug, Clone)]
enum FsBackend {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Arc<Mutex<FakeFs>>),
}

impl FileSystem {
    pub(super) const fn real() -> Self {
        Self { backend: FsBackend::Real }
    }

    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn fake(configuration: FakeFs) -> (Self, Arc<Mutex<FakeFs>>) {
        let state = Arc::new(Mutex::new(configuration));
        (
            Self {
                backend: FsBackend::Fake(Arc::clone(&state)),
            },
            state,
        )
    }

    fn create_dir_all(&self, path: &Path) -> std::io::Result<()> {
        match &self.backend {
            FsBackend::Real => std::fs::create_dir_all(path),
            #[cfg(any(test, feature = "test-util"))]
            FsBackend::Fake(fake) => fake
                .lock()
                .expect("fake filesystem mutex poisoned")
                .perform(PersistenceStage::CreateDirectory),
        }
    }

    fn create_temp(&self, parent: &Path) -> std::io::Result<PendingFile> {
        match &self.backend {
            FsBackend::Real => tempfile::NamedTempFile::new_in(parent).map(PendingFile::Real),
            #[cfg(any(test, feature = "test-util"))]
            FsBackend::Fake(fake) => {
                fake.lock()
                    .expect("fake filesystem mutex poisoned")
                    .perform(PersistenceStage::Create)?;
                Ok(PendingFile::Fake(FakeFile {
                    file_system: Arc::clone(fake),
                    bytes: Vec::new(),
                }))
            }
        }
    }

    fn wait(&self, attempt: u32) {
        match self.backend {
            FsBackend::Real => std::thread::sleep(std::time::Duration::from_millis(u64::from(attempt) * RETRY_MS)),
            #[cfg(any(test, feature = "test-util"))]
            FsBackend::Fake(_) => {}
        }
    }
}

enum PendingFile {
    Real(tempfile::NamedTempFile),
    #[cfg(any(test, feature = "test-util"))]
    Fake(FakeFile),
}

#[cfg(any(test, feature = "test-util"))]
struct FakeFile {
    file_system: Arc<Mutex<FakeFs>>,
    bytes: Vec<u8>,
}

impl std::io::Write for PendingFile {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Real(file) => file.as_file_mut().write(buf),
            #[cfg(any(test, feature = "test-util"))]
            Self::Fake(file) => {
                file.file_system
                    .lock()
                    .expect("fake filesystem mutex poisoned")
                    .perform(PersistenceStage::Write)?;
                file.bytes.extend_from_slice(buf);
                Ok(buf.len())
            }
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Real(file) => file.as_file_mut().flush(),
            #[cfg(any(test, feature = "test-util"))]
            Self::Fake(file) => file
                .file_system
                .lock()
                .expect("fake filesystem mutex poisoned")
                .perform(PersistenceStage::Flush),
        }
    }
}

impl PendingFile {
    fn sync_all(&self) -> std::io::Result<()> {
        match self {
            Self::Real(file) => file.as_file().sync_all(),
            #[cfg(any(test, feature = "test-util"))]
            Self::Fake(file) => file
                .file_system
                .lock()
                .expect("fake filesystem mutex poisoned")
                .perform(PersistenceStage::Sync),
        }
    }

    fn replace(self, path: &Path) -> Result<(), ReplaceError> {
        match self {
            Self::Real(file) => file.persist(path).map(|_persisted| ()).map_err(|rejected| ReplaceError {
                file: Self::Real(rejected.file),
                error: rejected.error,
            }),
            #[cfg(any(test, feature = "test-util"))]
            Self::Fake(file) => {
                let result = file
                    .file_system
                    .lock()
                    .expect("fake filesystem mutex poisoned")
                    .replace(&file.bytes);
                result.map_err(|error| ReplaceError {
                    file: Self::Fake(file),
                    error,
                })
            }
        }
    }
}

struct ReplaceError {
    file: PendingFile,
    error: std::io::Error,
}

#[cfg(any(test, feature = "test-util"))]
pub(super) mod persistence_stage {
    /// One fake-filesystem persistence operation that tests can observe or fail.
    #[non_exhaustive]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PersistenceStage {
        /// Parent-directory creation.
        CreateDirectory,
        /// Temporary-file creation.
        Create,
        /// Serialized-byte writing.
        Write,
        /// Buffered-byte flushing.
        Flush,
        /// Durable file synchronization.
        Sync,
        /// Atomic destination replacement.
        Replace,
    }
}
#[cfg(any(test, feature = "test-util"))]
use persistence_stage::PersistenceStage;

#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Default)]
pub(super) struct FakeFs {
    pub(super) fail_stage: Option<PersistenceStage>,
    pub(super) replace_failures_remaining: u32,
    pub(super) calls: Vec<PersistenceStage>,
    pub(super) snapshots: Vec<Vec<u8>>,
}

#[cfg(any(test, feature = "test-util"))]
impl FakeFs {
    fn perform(&mut self, stage: PersistenceStage) -> std::io::Result<()> {
        self.calls.push(stage);
        if self.fail_stage == Some(stage) {
            return Err(std::io::Error::other(format!("injected {stage:?} failure")));
        }
        Ok(())
    }

    fn replace(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.calls.push(PersistenceStage::Replace);
        if self.fail_stage == Some(PersistenceStage::Replace) || self.replace_failures_remaining > 0 {
            self.replace_failures_remaining = self.replace_failures_remaining.saturating_sub(1);
            return Err(std::io::Error::other("injected Replace failure"));
        }
        self.snapshots.push(bytes.to_vec());
        Ok(())
    }
}
