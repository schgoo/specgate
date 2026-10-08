//! Concrete crate-private boundaries for scoped operating-system interactions.

mod environment;
mod filesystem;
mod process;

use crate::error::{Error, ErrorKind};
#[cfg(any(test, feature = "test-util"))]
use std::collections::BTreeMap;
#[cfg(any(test, feature = "test-util"))]
use std::ffi::OsString;
#[cfg(any(test, feature = "test-util"))]
use std::path::PathBuf;

#[cfg(any(test, feature = "test-util"))]
const FAKE_PID: u32 = 0;

pub(crate) use environment::{CurrentDirectory, Environment, ProcessId};
#[cfg(any(test, feature = "test-util"))]
pub(crate) use filesystem::{FailureKey, FakeFs};
pub(crate) use filesystem::{Filesystem, UniqueDirectory};
#[cfg(any(test, feature = "test-util"))]
pub(crate) use process::FakeProcess;
#[cfg(any(test, feature = "test-util"))]
pub(crate) use process::ProcessOutput;
pub(crate) use process::{Process, Request as ProcessRequest};

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::System, message)
}

/// Groups concrete operating-system adapters used by discovery workflows.
#[derive(Clone)]
pub(crate) struct System {
    /// Filesystem operations.
    pub(crate) filesystem: Filesystem,
    /// Child-process execution.
    pub(crate) process: Process,
    /// Environment-variable access.
    pub(crate) environment: Environment,
    /// Current-directory access.
    pub(crate) current_directory: CurrentDirectory,
    /// Process-identity access.
    pub(crate) process_id: ProcessId,
}

impl System {
    #[cfg(any(test, feature = "test-util"))]
    /// Starts a deterministic system from fake filesystem and process adapters.
    pub(crate) fn builder(dependencies: impl Into<SystemDeps>) -> SystemBuilder {
        let dependencies = dependencies.into();
        SystemBuilder {
            filesystem: dependencies.filesystem,
            process: dependencies.process,
            environment: BTreeMap::new(),
            current_directory: Ok(PathBuf::from(".")),
            process_id: FAKE_PID,
        }
    }

    /// Creates adapters backed by the current operating system.
    pub(crate) fn real() -> Self {
        Self {
            filesystem: Filesystem::real(),
            process: Process::real(),
            environment: Environment::real(),
            current_directory: CurrentDirectory::real(),
            process_id: ProcessId::real(),
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// Required concrete dependencies for a deterministic system builder.
pub(crate) struct SystemDeps {
    filesystem: Filesystem,
    process: FakeProcess,
}

#[cfg(any(test, feature = "test-util"))]
impl From<(FakeFs, FakeProcess)> for SystemDeps {
    fn from((filesystem, process): (FakeFs, FakeProcess)) -> Self {
        Self {
            filesystem: Filesystem::fake(filesystem),
            process,
        }
    }
}

#[cfg(test)]
impl From<(Filesystem, FakeProcess)> for SystemDeps {
    fn from((filesystem, process): (Filesystem, FakeProcess)) -> Self {
        Self { filesystem, process }
    }
}

#[cfg(any(test, feature = "test-util"))]
/// Configures deterministic environment, directory, and process identity values.
pub(crate) struct SystemBuilder {
    filesystem: Filesystem,
    process: FakeProcess,
    environment: BTreeMap<OsString, OsString>,
    current_directory: Result<PathBuf, String>,
    process_id: u32,
}

#[cfg(any(test, feature = "test-util"))]
impl SystemBuilder {
    #[cfg(test)]
    /// Sets the complete fake environment.
    pub(crate) fn environment<K>(mut self, environment: BTreeMap<K, OsString>) -> Self
    where
        K: Into<OsString> + Ord,
    {
        self.environment = environment.into_iter().map(|(key, value)| (key.into(), value)).collect();
        self
    }

    #[cfg(test)]
    /// Sets the fake current-directory result.
    pub(crate) fn current_directory(mut self, current_directory: Result<PathBuf, String>) -> Self {
        self.current_directory = current_directory;
        self
    }

    #[cfg(test)]
    /// Sets the fake process identifier.
    pub(crate) fn process_id(mut self, process_id: u32) -> Self {
        self.process_id = process_id;
        self
    }

    /// Builds the configured fake system.
    pub(crate) fn build(self) -> System {
        System {
            filesystem: self.filesystem,
            process: Process::fake(self.process),
            environment: Environment::fake(self.environment),
            current_directory: CurrentDirectory::fake(self.current_directory),
            process_id: ProcessId::fake(self.process_id),
        }
    }
}
