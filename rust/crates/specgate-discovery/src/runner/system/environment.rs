//! Environment, current-directory, and process-identity access.

use super::failure;
use crate::error::Error;
#[cfg(any(test, feature = "test-util"))]
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
#[cfg(any(test, feature = "test-util"))]
use std::sync::Arc;

/// Reads process environment values through a real or deterministic fake adapter.
#[derive(Clone)]
pub(crate) struct Environment {
    inner: EnvironmentKind,
}
#[derive(Clone)]
enum EnvironmentKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Arc<BTreeMap<OsString, OsString>>),
}
impl Environment {
    pub(super) fn real() -> Self {
        Self {
            inner: EnvironmentKind::Real,
        }
    }
    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn fake(values: BTreeMap<OsString, OsString>) -> Self {
        Self {
            inner: EnvironmentKind::Fake(Arc::new(values)),
        }
    }
    /// Returns an operating-system value, or `None` when the variable is absent.
    pub(crate) fn var_os(&self, name: impl AsRef<OsStr>) -> Option<OsString> {
        let name = name.as_ref();
        match &self.inner {
            EnvironmentKind::Real => std::env::var_os(name),
            #[cfg(any(test, feature = "test-util"))]
            EnvironmentKind::Fake(values) => values.get(name).cloned(),
        }
    }
    /// Returns a Unicode value.
    ///
    /// Fails when the variable is absent or is not valid Unicode.
    pub(crate) fn var(&self, name: impl AsRef<OsStr>) -> Result<String, Error> {
        self.var_os(name)
            .ok_or_else(|| failure("environment variable not found"))?
            .into_string()
            .map_err(|_error| failure("environment variable is not Unicode"))
    }
}

/// Resolves the process current directory through a real or deterministic fake adapter.
#[derive(Clone)]
pub(crate) struct CurrentDirectory {
    inner: DirectoryKind,
}
#[derive(Clone)]
enum DirectoryKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(Result<PathBuf, String>),
}
impl CurrentDirectory {
    pub(super) fn real() -> Self {
        Self {
            inner: DirectoryKind::Real,
        }
    }
    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn fake(value: Result<PathBuf, String>) -> Self {
        Self {
            inner: DirectoryKind::Fake(value),
        }
    }
    /// Returns the current directory, propagating operating-system or injected failures.
    pub(crate) fn get(&self) -> Result<PathBuf, Error> {
        match &self.inner {
            DirectoryKind::Real => Ok(std::env::current_dir()?),
            #[cfg(any(test, feature = "test-util"))]
            DirectoryKind::Fake(value) => value.clone().map_err(failure),
        }
    }
}

/// Supplies the process identifier through a real or deterministic fake adapter.
#[derive(Clone)]
pub(crate) struct ProcessId {
    inner: IdKind,
}
#[derive(Clone)]
enum IdKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(u32),
}
impl ProcessId {
    pub(super) fn real() -> Self {
        Self { inner: IdKind::Real }
    }
    #[cfg(any(test, feature = "test-util"))]
    pub(super) fn fake(value: u32) -> Self {
        Self {
            inner: IdKind::Fake(value),
        }
    }
    /// Returns the real or injected process identifier.
    pub(crate) fn get(&self) -> u32 {
        match self.inner {
            IdKind::Real => std::process::id(),
            #[cfg(any(test, feature = "test-util"))]
            IdKind::Fake(value) => value,
        }
    }
}
