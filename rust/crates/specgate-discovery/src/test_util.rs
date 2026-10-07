//! Feature-gated harnesses for injecting binding and discovery system failures.
#![cfg(feature = "test-util")]
//!
//! Binding loading can be exercised without a real file:
//!
//! ```
//! use specgate_discovery::test_util::Binding;
//!
//! let binding = Binding::builder(
//!     "language: rust\ntargets:\n  default:\n    package_root: fixture\n",
//! )
//! .build()
//! .load("virtual/binding.yaml")?;
//! assert_eq!(binding.language.as_str(), "rust");
//! # Ok::<(), specgate_discovery::Error>(())
//! ```
//!
//! Discovery failures use a built [`Discovery`] and the ordinary
//! resolved target:
//!
//! ```
//! use specgate_discovery::binding::{ResolvedTarget, Target};
//! use specgate_discovery::binding::Language;
//! use specgate_discovery::test_util::Discovery;
//! use specgate_discovery::identity::{ComponentId, TargetName};
//! use std::path::PathBuf;
//!
//! let target = ResolvedTarget {
//!     binding_path: PathBuf::from("binding.yaml"),
//!     name: TargetName::from("default"),
//!     language: Language::Rust,
//!     target: Target::builder("candidate").build()?,
//! };
//! let system = Discovery::builder().process_failure("cargo unavailable").build();
//! assert!(system.discover(target, &[ComponentId::from("demo.component")]).is_err());
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

use crate::binding::{Binding as LoadedBinding, ResolvedTarget};
use crate::discovery::model::Batch;
use crate::error::Error;
use crate::runner::system::{FailureKey, FakeFs, FakeProcess, System as InnerSystem};
use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Built fake binding-loader filesystem used to load one or more bindings.
#[derive(Debug, Clone)]
pub struct Binding {
    canonicalize: Option<Result<PathBuf, String>>,
    read_to_string: Result<String, String>,
}

impl Binding {
    /// Start configuring a binding filesystem that returns `contents`.
    #[must_use]
    pub fn builder(contents: impl Into<String>) -> BindingBuilder {
        BindingBuilder {
            canonicalize: None,
            read_to_string: Ok(contents.into()),
        }
    }

    /// Load a binding through the injected filesystem results.
    ///
    /// # Errors
    ///
    /// Returns the same binding and typed system errors as [`crate::binding::load_binding`].
    pub fn load(&self, path: impl AsRef<Path>) -> Result<LoadedBinding, Error> {
        let path = path.as_ref();
        crate::binding::load_fake(
            path,
            self.canonicalize.clone().unwrap_or_else(|| Ok(path.to_path_buf())),
            self.read_to_string.clone(),
        )
    }
}

/// Builder for a feature-gated fake binding filesystem.
#[derive(Debug, Clone)]
pub struct BindingBuilder {
    canonicalize: Option<Result<PathBuf, String>>,
    read_to_string: Result<String, String>,
}

impl BindingBuilder {
    /// Return `path` from the injected canonicalization operation.
    #[must_use]
    pub fn canonical_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.canonicalize = Some(Ok(path.into()));
        self
    }

    /// Make canonicalization fail with `message`.
    #[must_use]
    pub fn canonicalization_failure(mut self, message: impl Into<String>) -> Self {
        self.canonicalize = Some(Err(message.into()));
        self
    }

    /// Make reading the binding fail with `message`.
    #[must_use]
    pub fn read_failure(mut self, message: impl Into<String>) -> Self {
        self.read_to_string = Err(message.into());
        self
    }

    /// Build the fake binding filesystem.
    #[must_use]
    pub fn build(self) -> Binding {
        Binding {
            canonicalize: self.canonicalize,
            read_to_string: self.read_to_string,
        }
    }
}

/// Built concrete fake system accepted by the canonical discovery workflow.
#[derive(Clone)]
pub struct Discovery {
    inner: InnerSystem,
}

impl std::fmt::Debug for Discovery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Discovery").finish_non_exhaustive()
    }
}

impl Discovery {
    /// Start configuring a fake discovery system.
    #[must_use]
    pub fn builder() -> DiscoveryBuilder {
        DiscoveryBuilder::default()
    }

    /// Discover components through this injected system.
    ///
    /// # Errors
    ///
    /// Returns the ordinary [`Error`] produced by resolved-target discovery.
    pub fn discover(&self, target: ResolvedTarget, components: impl AsRef<[crate::identity::ComponentId]>) -> Result<Batch, Error> {
        discover_with(target, components.as_ref(), self)
    }

    /// Resolve candidate and runtime package identity through the fake system.
    ///
    /// # Errors
    ///
    /// Returns the ordinary Cargo metadata and package-identity errors.
    pub fn package(&self, root: impl AsRef<Path>) -> Result<crate::runner::CandidatePackage, Error> {
        candidate_with(root, self)
    }
}

/// Discover components through a concrete fake system.
///
/// This is the feature-gated injection counterpart to [`crate::discover_many`].
///
/// # Errors
///
/// Returns the ordinary [`Error`] produced by resolved-target discovery.
pub fn discover_with(
    target: ResolvedTarget,
    components: impl AsRef<[crate::identity::ComponentId]>,
    system: &Discovery,
) -> Result<Batch, Error> {
    crate::discovery::discover_in(target, components.as_ref(), &system.inner)
}

/// Load, resolve, and discover one component through fake binding and system I/O.
///
/// This is the feature-gated injection counterpart to [`crate::discover`].
///
/// # Errors
///
/// Returns ordinary binding, target-selection, and discovery errors.
///
pub fn discover_fake(
    binding: &Binding,
    path: impl AsRef<Path>,
    target_name: Option<&crate::identity::TargetName>,
    component: crate::identity::ComponentId,
    system: &Discovery,
) -> Result<Batch, Error> {
    let target = binding
        .load(path)?
        .resolve_target(target_name.map(crate::identity::TargetName::as_str))?;
    system.discover(target, &[component])
}

/// Resolve candidate and runtime package identity through a concrete fake system.
///
/// This is the feature-gated injection counterpart to
/// [`crate::runner::candidate_package`].
///
/// # Errors
///
/// Returns ordinary Cargo metadata and package-identity errors.
pub fn candidate_with(root: impl AsRef<Path>, system: &Discovery) -> Result<crate::runner::CandidatePackage, Error> {
    crate::runner::candidate_in(root.as_ref(), &system.inner)
}

/// Builder for a feature-gated fake [`Discovery`].
#[derive(Debug, Default)]
pub struct DiscoveryBuilder {
    files: BTreeMap<PathBuf, Vec<u8>>,
    canonical: BTreeMap<PathBuf, Result<PathBuf, String>>,
    failures: Vec<(FailurePoint, String)>,
    process_failures: VecDeque<Result<crate::runner::system::ProcessOutput, String>>,
}

/// Filesystem operation exposed by the deterministic fake adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the fake filesystem protocol is closed so tests can match every operation"
)]
pub enum FileOperation {
    /// Canonicalize a path.
    Canonicalize,
    /// Create a directory tree.
    CreateDirectory,
    /// Read a directory.
    ReadDirectory,
    /// Read a UTF-8 file.
    ReadText,
    /// Write a file.
    Write,
    /// Remove a directory tree.
    RemoveDirectory,
    /// Allocate an invocation-unique directory.
    AllocateDirectory,
}

impl FileOperation {
    /// Return the internal fake-adapter protocol key.
    ///
    /// These values must stay synchronized with
    /// `support::system::filesystem::Operation::parse`; the system builder
    /// rejects unknown keys rather than silently ignoring a failure request.
    const fn key(self) -> &'static str {
        match self {
            Self::Canonicalize => "canonicalize",
            Self::CreateDirectory => "create_dir_all",
            Self::ReadDirectory => "read_dir",
            Self::ReadText => "read_to_string",
            Self::Write => "write",
            Self::RemoveDirectory => "remove_dir_all",
            Self::AllocateDirectory => "create_unique_dir",
        }
    }
}

/// Exact fake-filesystem operation at which to inject a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailurePoint {
    operation: FileOperation,
    file: Option<OsString>,
}

impl FailurePoint {
    /// Select every invocation of one operation.
    #[must_use]
    pub const fn operation(operation: FileOperation) -> Self {
        Self { operation, file: None }
    }

    /// Select one operation by a validated final path component.
    ///
    /// # Errors
    /// Returns a system-classified error for an empty component, a separator,
    /// or the colon reserved by the failure-key encoding.
    pub fn file(operation: FileOperation, file: impl Into<OsString>) -> Result<Self, Error> {
        let file = file.into();
        let spelling = file.to_string_lossy();
        if spelling.is_empty() || spelling.contains(['/', '\\', ':']) {
            return Err(Error::message(
                crate::error::ErrorKind::System,
                "failure filename must be one non-empty path component without ':', '/' or '\\'",
            ));
        }
        Ok(Self {
            operation,
            file: Some(file),
        })
    }

    fn key(&self) -> String {
        self.file.as_ref().map_or_else(
            || self.operation.key().to_owned(),
            |file| format!("{}:{}", self.operation.key(), file.to_string_lossy()),
        )
    }
}

impl DiscoveryBuilder {
    /// Provide one fake filesystem file.
    #[must_use]
    pub fn file(mut self, path: impl Into<PathBuf>, contents: impl AsRef<[u8]>) -> Self {
        self.files.insert(path.into(), contents.as_ref().to_vec());
        self
    }

    /// Provide one canonicalization result for an exact path.
    #[must_use]
    pub fn canonical_path(mut self, input: impl Into<PathBuf>, output: impl Into<PathBuf>) -> Self {
        self.canonical.insert(input.into(), Ok(output.into()));
        self
    }

    /// Queue a fake filesystem failure.
    ///
    #[must_use]
    pub fn filesystem_failure(mut self, point: FailurePoint, message: impl Into<String>) -> Self {
        self.failures.push((point, message.into()));
        self
    }

    /// Queue a process invocation failure.
    #[must_use]
    pub fn process_failure(mut self, message: impl Into<String>) -> Self {
        self.process_failures.push_back(Err(message.into()));
        self
    }

    /// Build the concrete fake system.
    ///
    /// # Panics
    ///
    /// Panics if the closed public operation enum is out of sync with the
    /// private fake-filesystem protocol.
    #[must_use]
    pub fn build(self) -> Discovery {
        let mut failures = BTreeMap::<FailureKey, VecDeque<String>>::new();
        for (point, message) in self.failures {
            let key_text = point.key();
            let key = FailureKey::parse(&key_text)
                .unwrap_or_else(|| panic!("FileOperation key '{key_text}' must match the internal fake filesystem protocol"));
            failures.entry(key).or_default().push_back(message);
        }
        for messages in failures.values_mut() {
            messages.shrink_to_fit();
        }
        let filesystem = FakeFs::builder()
            .files(self.files)
            .canonical(self.canonical)
            .failures(failures)
            .build();
        let mut process_failures = self.process_failures;
        process_failures.shrink_to_fit();
        let process = FakeProcess::queued(process_failures);
        Discovery {
            inner: InnerSystem::builder((filesystem, process)).build(),
        }
    }
}
