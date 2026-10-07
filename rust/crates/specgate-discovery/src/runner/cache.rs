//! Invocation-scoped discovery cache management.
//!

//! Invocation-unique cache allocation and cleanup.

use super::system;
use crate::error::{Error, ErrorKind};
use std::path::{Path, PathBuf};

// Stable namespace shared by cache discovery and cleanup across SpecGate versions.
const CACHE_NAMESPACE: &str = "specgate";

/// Sanitized namespace below the shared `SpecGate` cache directory.
///
/// Non-ASCII and punctuation characters become underscores. An empty scope
/// selects the namespace root; allocation still creates a unique child.
pub(crate) struct CacheScope(String);

impl CacheScope {
    /// Sanitize and retain a cache namespace.
    pub(crate) fn new(value: impl AsRef<str>) -> Self {
        Self(sanitize(value))
    }
}

/// Human-readable prefix for an invocation's unique directory.
///
/// Non-ASCII and punctuation characters become underscores. Empty labels are
/// valid; process identity and the allocator's collision suffix still ensure
/// that the resulting directory is invocation-unique.
pub(crate) struct CacheLabel(String);

impl CacheLabel {
    /// Sanitize and retain an allocation prefix.
    pub(crate) fn new(value: impl AsRef<str>) -> Self {
        Self(sanitize(value))
    }
}

/// Invocation cache directory removed automatically when dropped.
pub(crate) struct InvocationCache {
    directory: system::UniqueDirectory,
}

impl std::fmt::Debug for InvocationCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InvocationCache").field("path", &self.path()).finish()
    }
}

impl InvocationCache {
    /// Create one atomically allocated, operating-system-unique cache directory.
    ///
    /// # Errors
    ///
    /// Returns an error when no cache root is available or creation fails.
    #[cfg(any(test, feature = "test-util"))]
    #[cfg_attr(all(feature = "test-util", not(test)), expect(dead_code, reason = "feature-enabled test helper"))]
    pub(crate) fn create(scope: CacheScope, label: CacheLabel) -> Result<Self, Error> {
        Self::create_with(scope, label, &system::System::real())
    }

    /// Allocate a cache directory through the supplied real or fake system.
    ///
    /// `scope` selects the sanitized cache namespace and `label` prefixes the
    /// invocation-unique directory name.
    pub(crate) fn create_with(scope: CacheScope, label: CacheLabel, system: &system::System) -> Result<Self, Error> {
        let scope = scope.0;
        let label = label.0;
        let root = root_for(system)?;
        let parent = root.join(CACHE_NAMESPACE).join(scope);
        let prefix = format!("{label}-{}-", system.process_id.get());
        let directory = system.filesystem.unique_dir(&parent, &prefix).map_err(|source| {
            Error::cause(
                ErrorKind::System,
                format!("failed to create runtime cache directory {}: {source}", parent.display()),
                source,
            )
        })?;
        Ok(Self { directory })
    }

    #[must_use]
    /// Return the cache directory path.
    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }
}

impl AsRef<Path> for InvocationCache {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}
/// Stable Windows local-application cache path; changing it strands existing caches.
#[cfg(windows)]
const WINDOWS_PATH: [&str; 2] = ["SpecGate", "Cache"];
pub(super) fn root_for(system: &system::System) -> Result<PathBuf, Error> {
    let env_path = |name: &str| system.environment.var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from);
    if let Some(path) = env_path("SPECGATE_CACHE_DIR") {
        return Ok(path);
    }
    #[cfg(windows)]
    if let Some(path) = env_path("LOCALAPPDATA") {
        return Ok(path.join(WINDOWS_PATH[0]).join(WINDOWS_PATH[1]));
    }
    if let Some(path) = env_path("XDG_CACHE_HOME") {
        return Ok(path);
    }
    if let Some(path) = env_path("HOME") {
        // Conventional Unix HOME fallback; changing this path would strand existing SpecGate caches.
        return Ok(path.join(".cache"));
    }
    Err(Error::message(
        ErrorKind::System,
        "no operating-system cache directory is available; set SPECGATE_CACHE_DIR",
    ))
}

fn sanitize(value: impl AsRef<str>) -> String {
    value
        .as_ref()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::system::{FailureKey, FakeFs, FakeProcess, Filesystem, System};
    use std::collections::{BTreeMap, VecDeque};

    fn linked_system(filesystem: Filesystem) -> System {
        System::builder((filesystem, FakeProcess::default()))
            .environment(BTreeMap::from([("SPECGATE_CACHE_DIR".to_string(), "cache".into())]))
            .current_directory(Ok(PathBuf::from("cwd")))
            .process_id(41)
            .build()
    }

    fn bare_system(environment: BTreeMap<String, std::ffi::OsString>) -> System {
        System::builder((FakeFs::default(), FakeProcess::default()))
            .environment(environment)
            .current_directory(Ok(PathBuf::from("cwd")))
            .process_id(41)
            .build()
    }

    #[test]
    fn root_precedence() {
        let system = bare_system(BTreeMap::from([
            ("SPECGATE_CACHE_DIR".to_string(), "explicit".into()),
            ("LOCALAPPDATA".to_string(), "local".into()),
            ("XDG_CACHE_HOME".to_string(), "xdg".into()),
            ("HOME".to_string(), "home".into()),
        ]));
        assert_eq!(root_for(&system).unwrap(), PathBuf::from("explicit"));
    }

    #[test]
    fn missing_root() {
        let error = root_for(&bare_system(BTreeMap::new())).unwrap_err();
        assert!(error.diagnostic().contains("SPECGATE_CACHE_DIR"));
    }

    #[test]
    fn allocation_failure() {
        assert_eq!(CacheScope::new("scope/name").0, "scope_name");
        assert_eq!(CacheLabel::new("run:name").0, "run_name");

        let filesystem = Filesystem::fake(
            FakeFs::builder()
                .unique_candidates(VecDeque::from([Err("permission denied".to_string())]))
                .build(),
        );
        let error = InvocationCache::create_with(CacheScope::new("scope"), CacheLabel::new("run"), &linked_system(filesystem)).unwrap_err();
        assert!(error.diagnostic().contains("permission denied"));
    }

    #[test]
    fn drop_cleanup() {
        let filesystem = Filesystem::fake(
            FakeFs::builder()
                .failures(BTreeMap::from([(
                    FailureKey::test("remove_dir_all"),
                    VecDeque::from(["locked".to_string()]),
                )]))
                .unique_candidates(VecDeque::from([Ok(PathBuf::from("cache/specgate/scope/run"))]))
                .build(),
        );
        let state = filesystem.fake_state();
        let system = linked_system(filesystem);
        let cache = InvocationCache::create_with(CacheScope::new("scope"), CacheLabel::new("run"), &system).unwrap();
        drop(cache);
        assert!(state.borrow().removed().is_empty());
    }
}
