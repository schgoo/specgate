//! Filesystem, environment, and scratch-allocation boundary.
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
#[cfg(any(test, feature = "test-util"))]
use std::path::PathBuf;
#[cfg(any(test, feature = "test-util"))]
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex},
};

/// Filesystem, environment, and scratch service with real and fake backends.
///
/// Real scratch guards own invocation-unique temporary directories and remove
/// only their own roots. With `test-util`, tests can seed isolated files and
/// environment values, inject failures, and verify scratch ownership:
///
/// ```
/// # #[cfg(feature = "test-util")] {
/// use specgate_cli::CommandEnvironment;
/// let system = CommandEnvironment::fake();
/// system.seed_file("binding.yaml", b"language: rust");
/// system.fail_next("injected failure");
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct CommandEnvironment {
    inner: Kind,
}
#[derive(Clone, Debug)]
enum Kind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Arc<Mutex<Fake>>),
}
#[cfg(any(test, feature = "test-util"))]
#[derive(Debug, Default)]
struct Fake {
    failures: VecDeque<io::Error>,
    files: HashMap<PathBuf, Vec<u8>>,
    environment: HashMap<OsString, OsString>,
    directories: HashSet<PathBuf>,
    owned_scratch: HashSet<PathBuf>,
    next_scratch: u64,
}

/// Owning invocation-unique scratch directory guard.
#[derive(Debug)]
pub(crate) struct Scratch {
    inner: ScratchKind,
}
#[derive(Debug)]
enum ScratchKind {
    Real(tempfile::TempDir),
    #[cfg(any(test, feature = "test-util"))]
    Fake {
        path: PathBuf,
        system: Arc<Mutex<Fake>>,
    },
}
impl Scratch {
    /// Borrow the allocated root.
    pub(crate) fn path(&self) -> &Path {
        match &self.inner {
            ScratchKind::Real(inner) => inner.path(),
            #[cfg(any(test, feature = "test-util"))]
            ScratchKind::Fake { path, .. } => path,
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
impl Drop for Scratch {
    fn drop(&mut self) {
        let ScratchKind::Fake { path, system } = &self.inner else {
            return;
        };
        let mut fake = system.lock().unwrap_or_else(|_poisoned| std::process::abort());
        if fake.owned_scratch.remove(path) {
            fake.directories.remove(path);
            fake.files.retain(|candidate, _| !candidate.starts_with(path));
        }
    }
}

impl CommandEnvironment {
    /// Select native system services.
    #[must_use]
    pub const fn real() -> Self {
        Self { inner: Kind::Real }
    }
    fn take_error(&self) -> Option<io::Error> {
        match &self.inner {
            Kind::Real => None,
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => fake
                .lock()
                .expect("fake system lock poisoned")
                .failures
                .pop_front()
                .map(io::Error::other),
        }
    }
    /// Read a complete file.
    pub(crate) fn read(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        match &self.inner {
            Kind::Real => fs::read(path),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => fake
                .lock()
                .expect("fake system lock poisoned")
                .files
                .get(path.as_ref())
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "fake file not found")),
        }
    }
    /// Replace a complete file.
    pub(crate) fn write(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        match &self.inner {
            Kind::Real => fs::write(path, bytes),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => {
                fake.lock()
                    .expect("fake system lock poisoned")
                    .files
                    .insert(path.as_ref().to_path_buf(), bytes.as_ref().to_vec());
                Ok(())
            }
        }
    }
    /// Recursively create a directory.
    pub(crate) fn create_dir_all(&self, path: impl AsRef<Path>) -> io::Result<()> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        match &self.inner {
            Kind::Real => fs::create_dir_all(path),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => {
                fake.lock()
                    .expect("fake system lock poisoned")
                    .directories
                    .insert(path.as_ref().to_path_buf());
                Ok(())
            }
        }
    }
    /// Remove a file.
    pub(crate) fn remove_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        match &self.inner {
            Kind::Real => fs::remove_file(path),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => {
                fake.lock().expect("fake system lock poisoned").files.remove(path.as_ref());
                Ok(())
            }
        }
    }
    /// Test whether a regular file exists.
    pub(crate) fn is_file(&self, path: impl AsRef<Path>) -> bool {
        if self.take_error().is_some() {
            return false;
        }
        match &self.inner {
            Kind::Real => path.as_ref().is_file(),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => fake.lock().expect("fake system lock poisoned").files.contains_key(path.as_ref()),
        }
    }
    /// Read one environment value without Unicode conversion.
    pub(crate) fn environment(&self, name: impl AsRef<OsStr>) -> Option<OsString> {
        self.take_error().is_none().then_some(())?;
        match &self.inner {
            Kind::Real => std::env::var_os(name.as_ref()),
            #[cfg(any(test, feature = "test-util"))]
            Kind::Fake(fake) => fake
                .lock()
                .expect("fake system lock poisoned")
                .environment
                .get(name.as_ref())
                .cloned(),
        }
    }
    /// Allocate a collision-safe scratch root whose guard cleans up only that root.
    pub(crate) fn scratch(&self, parent: Option<impl AsRef<Path>>, prefix: impl AsRef<OsStr>) -> io::Result<Scratch> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        #[cfg(any(test, feature = "test-util"))]
        if let Kind::Fake(fake) = &self.inner {
            let parent = parent.as_ref().map_or_else(|| Path::new("fake-scratch"), AsRef::as_ref);
            let prefix = prefix.as_ref().to_string_lossy();
            let path = loop {
                let mut state = fake.lock().expect("fake system lock poisoned");
                let identity = state.next_scratch;
                state.next_scratch = state.next_scratch.checked_add(1).expect("fake scratch identity overflow");
                let candidate = parent.join(format!("{prefix}{identity}"));
                if state.directories.insert(candidate.clone()) {
                    state.owned_scratch.insert(candidate.clone());
                    break candidate;
                }
            };
            return Ok(Scratch {
                inner: ScratchKind::Fake {
                    path,
                    system: Arc::clone(fake),
                },
            });
        }
        let mut builder = tempfile::Builder::new();
        builder.prefix(prefix.as_ref());
        let inner = if let Some(parent) = parent {
            builder.tempdir_in(parent)?
        } else {
            builder.tempdir()?
        };
        Ok(Scratch {
            inner: ScratchKind::Real(inner),
        })
    }
    /// Atomically publish bytes beside the final path using a unique temporary file.
    pub(crate) fn publish(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>, prefix: impl AsRef<OsStr>) -> io::Result<()> {
        if let Some(error) = self.take_error() {
            return Err(error);
        }
        let path = path.as_ref();
        #[cfg(any(test, feature = "test-util"))]
        if let Kind::Fake(fake) = &self.inner {
            fake.lock()
                .expect("fake system lock poisoned")
                .files
                .insert(path.to_path_buf(), bytes.as_ref().to_vec());
            return Ok(());
        }
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::Builder::new().prefix(prefix.as_ref()).tempfile_in(parent)?;
        temporary.write_all(bytes.as_ref())?;
        temporary.as_file_mut().sync_all()?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        temporary.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    /// Construct a fake that delegates successful operations to isolated test paths.
    #[cfg(any(test, feature = "test-util"))]
    #[must_use]
    pub fn fake() -> Self {
        Self {
            inner: Kind::Fake(Arc::new(Mutex::new(Fake::default()))),
        }
    }
    /// Seed one fake file without performing a syscall.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn seed_file(&self, path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) {
        let Kind::Fake(fake) = &self.inner else {
            panic!("seed_file requires fake system")
        };
        fake.lock()
            .expect("fake system lock poisoned")
            .files
            .insert(path.as_ref().to_path_buf(), bytes.as_ref().to_vec());
    }
    /// Seed one fake environment value without reading process state.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn seed_environment(&self, name: impl Into<OsString>, value: impl Into<OsString>) {
        let Kind::Fake(fake) = &self.inner else {
            panic!("seed_environment requires fake system")
        };
        fake.lock()
            .expect("fake system lock poisoned")
            .environment
            .insert(name.into(), value.into());
    }
    /// Seed one pre-existing fake directory.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn seed_directory(&self, path: impl AsRef<Path>) {
        let Kind::Fake(fake) = &self.inner else {
            panic!("seed_directory requires fake system")
        };
        fake.lock()
            .expect("fake system lock poisoned")
            .directories
            .insert(path.as_ref().to_path_buf());
    }
    /// Test whether a fake directory currently exists.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn is_directory(&self, path: impl AsRef<Path>) -> bool {
        let Kind::Fake(fake) = &self.inner else {
            panic!("is_directory requires fake system")
        };
        fake.lock().expect("fake system lock poisoned").directories.contains(path.as_ref())
    }

    /// Inject the next system-operation failure.
    ///
    /// # Panics
    /// Panics when called on a real backend or when fake state is poisoned.
    #[cfg(any(test, feature = "test-util"))]
    pub fn fail_next(&self, message: impl AsRef<str>) {
        let Kind::Fake(fake) = &self.inner else {
            panic!("fail_next requires fake system")
        };
        fake.lock()
            .expect("fake system lock poisoned")
            .failures
            .push_back(io::Error::other(message.as_ref()));
    }
}
