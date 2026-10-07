//! Strict CTSC target-binding parsing and path resolution.
//!
//! # Examples
//! ```
//! use specgate_discovery::binding::parse_binding;
//! let binding = parse_binding("specgate.yaml", "language: rust\ntargets:\n  default:\n    package_root: .\n").unwrap();
//! assert_eq!(binding.language.as_str(), "rust");
//! assert_eq!(binding.resolve_target(None).unwrap().name.as_str(), "default");
//! ```
//!
//! Builders validate required target state and retain structured output declarations:
//! ```
//! use specgate_discovery::binding::{OutputFormat, Runtime, Target, TargetOutputs};
//! let target = Target::builder("crate")
//!     .runtime(Runtime::Tokio)
//!     .outputs(Some(TargetOutputs { file: Some("result.json".into()), stdout: Some(OutputFormat::Json) }))
//!     .build().unwrap();
//! assert_eq!(target.runtime, Runtime::Tokio);
//! assert!(Target::builder("").build().is_err());
//! ```
//!
//! Explicit target selection reports unknown names rather than silently falling back:
//! ```
//! use specgate_discovery::binding::parse_binding;
//! let binding = parse_binding("specgate.yaml", "language: rust\ntargets:\n  first:\n    package_root: .\n").unwrap();
//! assert_eq!(binding.resolve_target(None).unwrap().name.as_str(), "first");
//! assert!(binding.resolve_target(Some("missing")).is_err());
//! ```

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorKind};
use crate::identity::TargetName;

// Binding schema protocol values shared with discovery dispatch.
const RUST_LANGUAGE: &str = "rust";
const CSHARP_LANGUAGE: &str = "csharp";
/// Reserved schema target selected when callers omit an explicit target name.
const DEFAULT_TARGET: &str = "default";

/// Async runtime selected by a Rust target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(clippy::exhaustive_enums, reason = "the binding schema defines a closed runtime vocabulary")]
pub enum Runtime {
    /// Smol executor.
    #[default]
    Smol,
    /// Tokio executor.
    Tokio,
}

/// Structured command output declaration retained by CTSC target bindings.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[expect(
    clippy::exhaustive_structs,
    reason = "binding DTOs intentionally expose the complete parsed schema"
)]
pub struct TargetOutputs {
    /// Optional output file declaration.
    pub file: Option<PathBuf>,
    /// Optional structured standard-output format.
    pub stdout: Option<OutputFormat>,
}

/// Supported structured stdout formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "the binding schema defines a closed output-format vocabulary")]
pub enum OutputFormat {
    /// JSON output.
    Json,
    /// YAML output.
    Yaml,
}

/// One unresolved target exactly as represented in YAML.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    package_root: PathBuf,
    #[serde(default)]
    runtime: RuntimeFile,
    framework: Option<String>,
    command: Option<String>,
    outputs: Option<WireOutputs>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RuntimeFile {
    #[default]
    Smol,
    Tokio,
}

impl From<RuntimeFile> for Runtime {
    fn from(value: RuntimeFile) -> Self {
        match value {
            RuntimeFile::Smol => Self::Smol,
            RuntimeFile::Tokio => Self::Tokio,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireOutputs {
    file: Option<PathBuf>,
    stdout: Option<WireFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireFormat {
    Json,
    Yaml,
}

impl From<WireOutputs> for TargetOutputs {
    fn from(value: WireOutputs) -> Self {
        Self {
            file: value.file,
            stdout: value.stdout.map(|format| match format {
                WireFormat::Json => OutputFormat::Json,
                WireFormat::Yaml => OutputFormat::Yaml,
            }),
        }
    }
}

/// One resolved implementation target.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "binding DTOs intentionally expose the complete parsed schema"
)]
pub struct Target {
    /// Resolved package root.
    pub package_root: PathBuf,
    /// Selected async runtime.
    pub runtime: Runtime,
    /// Optional target framework.
    pub framework: Option<String>,
    /// Optional target command.
    pub command: Option<String>,
    /// Optional structured output declaration.
    pub outputs: Option<TargetOutputs>,
}

/// Builder for a resolved implementation [`Target`].
#[must_use]
#[derive(Debug)]
pub struct TargetBuilder {
    package_root: PathBuf,
    runtime: Runtime,
    framework: Option<String>,
    command: Option<String>,
    outputs: Option<TargetOutputs>,
}

impl Target {
    /// Start a target builder with the required package root.
    pub fn builder(package_root: impl Into<PathBuf>) -> TargetBuilder {
        TargetBuilder {
            package_root: package_root.into(),
            runtime: Runtime::default(),
            framework: None,
            command: None,
            outputs: None,
        }
    }
}

impl TargetBuilder {
    /// Select the async runtime.
    pub const fn runtime(mut self, runtime: Runtime) -> Self {
        self.runtime = runtime;
        self
    }
    /// Set the optional target framework.
    pub fn framework(mut self, framework: Option<String>) -> Self {
        self.framework = framework;
        self
    }
    /// Set the optional target command.
    pub fn command(mut self, command: Option<String>) -> Self {
        self.command = command;
        self
    }
    /// Set the optional output declaration.
    pub fn outputs(mut self, outputs: Option<TargetOutputs>) -> Self {
        self.outputs = outputs;
        self
    }
    /// Build the target.
    ///
    /// # Errors
    ///
    /// Returns an error when the package root is empty.
    pub fn build(self) -> Result<Target, Error> {
        if self.package_root.as_os_str().is_empty() {
            return Err(Error::message(ErrorKind::Binding, "target package_root must not be empty"));
        }
        Ok(Target {
            package_root: self.package_root,
            runtime: self.runtime,
            framework: self.framework,
            command: self.command,
            outputs: self.outputs,
        })
    }
}

/// A parsed binding with every path resolved relative to the binding file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "binding DTOs intentionally expose the complete parsed schema"
)]
pub struct Binding {
    /// Absolute binding-file path.
    pub path: PathBuf,
    /// Implementation language.
    pub language: Language,
    /// Named resolved targets.
    pub targets: BTreeMap<TargetName, Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    language: LanguageFile,
    targets: BTreeMap<String, TargetFile>,
}

/// Supported implementation language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the binding schema defines a closed implementation-language vocabulary"
)]
pub enum Language {
    /// Rust link-time metadata discovery.
    Rust,
    /// C# compiled-assembly reflection.
    CSharp,
}

impl Language {
    /// Return the binding-schema spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => RUST_LANGUAGE,
            Self::CSharp => CSHARP_LANGUAGE,
        }
    }
}

impl AsRef<str> for Language {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for Language {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LanguageFile {
    Rust,
    CSharp,
}

impl From<LanguageFile> for Language {
    fn from(value: LanguageFile) -> Self {
        match value {
            LanguageFile::Rust => Self::Rust,
            LanguageFile::CSharp => Self::CSharp,
        }
    }
}

struct Filesystem {
    inner: FilesystemKind,
}

enum FilesystemKind {
    Real,
    #[cfg(any(test, feature = "test-util"))]
    Fake(FakeFs),
}

#[cfg(any(test, feature = "test-util"))]
struct FakeFs {
    canonicalize: Result<PathBuf, String>,
    read_to_string: Result<String, String>,
}

impl Filesystem {
    fn real() -> Self {
        Self {
            inner: FilesystemKind::Real,
        }
    }

    fn canonicalize(&self, path: impl AsRef<Path>) -> Result<PathBuf, Error> {
        let path = path.as_ref();
        match &self.inner {
            FilesystemKind::Real => Ok(std::fs::canonicalize(path)?),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(fake) => fake
                .canonicalize
                .clone()
                .map_err(|message| Error::message(ErrorKind::System, message)),
        }
    }

    fn read_to_string(&self, path: impl AsRef<Path>) -> Result<String, Error> {
        let path = path.as_ref();
        match &self.inner {
            FilesystemKind::Real => Ok(std::fs::read_to_string(path)?),
            #[cfg(any(test, feature = "test-util"))]
            FilesystemKind::Fake(fake) => fake
                .read_to_string
                .clone()
                .map_err(|message| Error::message(ErrorKind::System, message)),
        }
    }
}

/// A selected binding target.
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_structs,
    reason = "resolved binding DTOs intentionally expose all selected target evidence"
)]
pub struct ResolvedTarget {
    /// Absolute source binding path.
    pub binding_path: PathBuf,
    /// Selected target name.
    pub name: TargetName,
    /// Implementation language.
    pub language: Language,
    /// Selected target configuration.
    pub target: Target,
}

impl Binding {
    /// Select a named target, or `default` and then the first sorted target.
    ///
    /// # Errors
    ///
    /// Returns an actionable error for an unknown target or when the binding
    /// has no target that can be selected.
    pub fn resolve_target(&self, requested: Option<&str>) -> Result<ResolvedTarget, Error> {
        let (name, target) = match requested.filter(|name| !name.is_empty()) {
            Some(name) => self.targets.get_key_value(name).ok_or_else(|| {
                Error::message(
                    ErrorKind::Binding,
                    format!(
                        "target '{name}' not found in binding; available targets: {}",
                        self.targets.keys().map(TargetName::as_str).collect::<Vec<_>>().join(", ")
                    ),
                )
            })?,
            None => self
                .targets
                .get_key_value(DEFAULT_TARGET)
                .or_else(|| self.targets.first_key_value())
                .ok_or_else(|| Error::message(ErrorKind::Binding, format!("binding '{}' contains no targets", self.path.display())))?,
        };
        Ok(ResolvedTarget {
            binding_path: self.path.clone(),
            name: name.clone(),
            language: self.language,
            target: target.clone(),
        })
    }
}

/// Load and validate a binding file.
///
/// # Errors
///
/// Rejects I/O/YAML errors, unknown fields, unsupported languages, empty
/// target sets and empty paths.
pub fn load_binding(path: impl AsRef<Path>) -> Result<Binding, Error> {
    load_with(path.as_ref(), &Filesystem::real())
}

#[cfg(feature = "test-util")]
/// Load a binding through deterministic filesystem outcomes.
///
/// This feature-gated entry point lets downstream tests exercise both success
/// and failure paths without touching the host filesystem.
///
/// # Errors
///
/// Returns the supplied filesystem failure or an ordinary binding error.
pub fn load_fake(
    path: impl AsRef<Path>,
    canonicalize: Result<PathBuf, String>,
    read_to_string: Result<String, String>,
) -> Result<Binding, Error> {
    load_with(
        path.as_ref(),
        &Filesystem {
            inner: FilesystemKind::Fake(FakeFs {
                canonicalize,
                read_to_string,
            }),
        },
    )
}

fn load_with(path: &Path, filesystem: &Filesystem) -> Result<Binding, Error> {
    let path = filesystem.canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = filesystem.read_to_string(&path).map_err(|source| {
        Error::cause(
            ErrorKind::Binding,
            format!("binding '{}' not found or invalid: {source}", path.display()),
            source,
        )
    })?;
    parse_binding(&path, &text)
}

/// Parse and resolve a binding from supplied YAML content.
///
/// Relative package roots are resolved against `path` exactly as they are in
/// [`load_binding`], without reading or canonicalizing the filesystem.
///
/// # Errors
///
/// Rejects YAML errors, unknown fields, unsupported languages, empty target
/// sets and empty paths.
pub fn parse_binding(path: impl AsRef<Path>, text: impl AsRef<str>) -> Result<Binding, Error> {
    let path = path.as_ref();
    let file: BindingFile = serde_yaml::from_str(text.as_ref()).map_err(|source| {
        Error::cause(
            ErrorKind::Binding,
            format!("binding '{}' not found or invalid: {source}", path.display()),
            source,
        )
    })?;
    if file.targets.is_empty() {
        return Err(Error::message(
            ErrorKind::Binding,
            format!("binding '{}' contains no targets", path.display()),
        ));
    }

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let targets = file
        .targets
        .into_iter()
        .map(|(name, target)| {
            if name.trim().is_empty() {
                return Err(Error::message(
                    ErrorKind::Binding,
                    format!("binding '{}' contains an empty target name", path.display()),
                ));
            }
            if target.package_root.as_os_str().is_empty() {
                return Err(Error::message(
                    ErrorKind::Binding,
                    format!("binding target '{name}' has an empty package_root"),
                ));
            }
            let package_root = normalize(base.join(&target.package_root));
            Ok((
                TargetName::from(name),
                Target::builder(package_root)
                    .runtime(target.runtime.into())
                    .framework(target.framework)
                    .command(target.command)
                    .outputs(target.outputs.map(TargetOutputs::from))
                    .build()?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    Ok(Binding {
        path: path.to_path_buf(),
        language: file.language.into(),
        targets,
    })
}

/// Load a binding and select one target using the shared defaulting rules.
///
/// # Errors
///
/// Returns binding parsing or target-selection errors.
pub fn resolve_target(path: impl AsRef<Path>, requested: Option<&str>) -> Result<ResolvedTarget, Error> {
    load_binding(path)?.resolve_target(requested)
}

fn normalize(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            std::path::Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct BindingFixture {
        _cache: crate::runner::cache::InvocationCache,
        path: PathBuf,
    }

    fn binding_file(yaml: &str) -> BindingFixture {
        let cache = crate::runner::cache::InvocationCache::create(
            crate::runner::cache::CacheScope::new("binding-tests"),
            crate::runner::cache::CacheLabel::new("binding"),
        )
        .unwrap();
        let path = cache.path().join("binding.yaml");
        std::fs::write(&path, yaml).unwrap();
        BindingFixture { _cache: cache, path }
    }

    #[test]
    fn parses_all_retained_target_fields() {
        let path = binding_file(
            "language: csharp\ntargets:\n  default:\n    package_root: fixture\n    runtime: tokio\n    framework: net8.0\n    command: dotnet run\n    outputs: { file: result.json, stdout: json }\n",
        );
        let binding = load_binding(&path.path).unwrap();
        let resolved = binding.resolve_target(None).unwrap();
        assert_eq!(resolved.name, "default");
        assert_eq!(resolved.target.runtime, Runtime::Tokio);
        assert_eq!(resolved.target.framework.as_deref(), Some("net8.0"));
        assert_eq!(resolved.target.outputs.unwrap().stdout, Some(OutputFormat::Json));
    }

    #[test]
    fn rejects_unknown_fields() {
        let unknown = binding_file("language: rust\ntargets:\n  default:\n    package_root: .\n    surprise: true\n");
        assert!(load_binding(&unknown.path).unwrap_err().contains("unknown field"));
    }

    #[test]
    fn content_and_path_loading_have_identical_results() {
        let yaml = "language: rust\ntargets:\n  default:\n    package_root: fixture\n";
        let fixture = binding_file(yaml);
        let loaded = load_binding(&fixture.path).unwrap();
        let parsed = parse_binding(&loaded.path, yaml).unwrap();
        assert_eq!(loaded, parsed);
    }

    #[test]
    fn lexical_normalization_preserves_unresolved_parents() {
        assert_eq!(normalize(Path::new("../a/./b/../c")), PathBuf::from("../a/c"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
    }

    #[test]
    fn fake_filesystem_injects_canonicalization_and_read_failures() {
        let original = Path::new("relative/binding.yaml");
        let canonical = PathBuf::from("canonical/binding.yaml");
        let yaml = "language: rust\ntargets:\n  default:\n    package_root: fixture\n";
        let filesystem = Filesystem {
            inner: FilesystemKind::Fake(FakeFs {
                canonicalize: Ok(canonical.clone()),
                read_to_string: Ok(yaml.to_string()),
            }),
        };
        let loaded = load_with(original, &filesystem).unwrap();
        assert_eq!(loaded.path, canonical);

        let filesystem = Filesystem {
            inner: FilesystemKind::Fake(FakeFs {
                canonicalize: Err("canonicalization denied".to_string()),
                read_to_string: Err("read denied".to_string()),
            }),
        };
        assert_eq!(
            load_with(original, &filesystem).unwrap_err(),
            "binding 'relative/binding.yaml' not found or invalid: read denied"
        );
    }
}
