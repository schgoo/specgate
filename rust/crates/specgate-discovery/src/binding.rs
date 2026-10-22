//! Strict CTSC implementation-binding parsing and path resolution.
//!
//! A binding file maps each component to the implementations that provide it:
//! `components.<componentId>.implementations.<label>`. Every implementation
//! names its `language`, the `sources` directory discovery and replay build,
//! and exactly one way to run its `tests`.
//!
//! # Examples
//! ```
//! use specgate_discovery::binding::{Language, parse_binding};
//! let yaml = "components:\n  example.orders:\n    implementations:\n      main:\n        language: rust\n        sources: orders\n        tests:\n          command: cargo test\n";
//! let binding = parse_binding("specgate.yaml", yaml).unwrap();
//! let resolved = binding.resolve_implementation("example.orders", None).unwrap();
//! assert_eq!(resolved.label.as_str(), "main");
//! assert_eq!(resolved.language(), Language::Rust);
//! ```
//!
//! Selection reports unknown or ambiguous choices rather than silently falling back:
//! ```
//! use specgate_discovery::binding::parse_binding;
//! let yaml = "components:\n  example.orders:\n    implementations:\n      a: { language: rust, sources: a, tests: { command: t } }\n      b: { language: rust, sources: b, tests: { command: t } }\n";
//! let binding = parse_binding("specgate.yaml", yaml).unwrap();
//! assert!(binding.resolve_implementation("example.orders", None).is_err());
//! assert!(binding.resolve_implementation("example.orders", Some("b")).is_ok());
//! assert!(binding.resolve_implementation("example.users", None).is_err());
//! ```

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorKind};
use crate::identity::{ComponentId, ImplementationLabel};

// Binding schema protocol values shared with discovery dispatch.
const RUST_LANGUAGE: &str = "rust";
const CSHARP_LANGUAGE: &str = "csharp";

/// Async runtime selected by a Rust implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[expect(clippy::exhaustive_enums, reason = "the binding schema defines a closed runtime vocabulary")]
pub enum Runtime {
    /// Select the only supported direct candidate dependency.
    #[default]
    Auto,
    /// Arty executor.
    Arty,
    /// Smol executor.
    Smol,
    /// Tokio executor.
    Tokio,
}

/// How an implementation's tests are run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(clippy::exhaustive_enums, reason = "the binding schema defines exactly two test declarations")]
pub enum Tests {
    /// Test project paths, resolved relative to the binding file.
    Projects(Vec<PathBuf>),
    /// A shell command that runs the tests.
    Command(String),
}

/// Language-specific implementation settings.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the binding schema defines a closed implementation-language vocabulary"
)]
pub enum LanguageOptions {
    /// Settings of the `rust` block.
    Rust {
        /// Selected async runtime.
        runtime: Runtime,
        /// Optional crate-rooted function that owns replay execution.
        executor: Option<String>,
    },
    /// Settings of the `csharp` block.
    CSharp {
        /// Optional target framework.
        framework: Option<String>,
    },
}

/// One implementation of a component with every path resolved relative to the binding file.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "binding DTOs intentionally expose the complete parsed schema"
)]
pub struct Implementation {
    /// Directory that discovery and replay build.
    pub sources: PathBuf,
    /// How the implementation's tests are run.
    pub tests: Tests,
    /// Language-specific settings; the variant fixes the implementation language.
    pub options: LanguageOptions,
}

impl Implementation {
    /// Create an implementation, validating the declarations.
    ///
    /// # Errors
    ///
    /// Returns an error when `sources` is empty, a test project path is empty,
    /// the project list is empty, or the test command is blank.
    pub fn new(sources: impl Into<PathBuf>, tests: Tests, options: LanguageOptions) -> Result<Self, Error> {
        let sources = sources.into();
        if sources.as_os_str().is_empty() {
            return Err(Error::message(ErrorKind::Binding, "implementation sources must not be empty"));
        }
        match &tests {
            Tests::Projects(projects) if projects.is_empty() => {
                return Err(Error::message(ErrorKind::Binding, "tests.projects must list at least one path"));
            }
            Tests::Projects(projects) if projects.iter().any(|project| project.as_os_str().is_empty()) => {
                return Err(Error::message(ErrorKind::Binding, "tests.projects must not contain an empty path"));
            }
            Tests::Command(command) if command.trim().is_empty() => {
                return Err(Error::message(ErrorKind::Binding, "tests.command must not be blank"));
            }
            Tests::Projects(_) | Tests::Command(_) => {}
        }
        Ok(Self { sources, tests, options })
    }

    /// The implementation language.
    #[must_use]
    pub const fn language(&self) -> Language {
        match self.options {
            LanguageOptions::Rust { .. } => Language::Rust,
            LanguageOptions::CSharp { .. } => Language::CSharp,
        }
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
    /// Implementations of every component, keyed by component and label.
    pub components: BTreeMap<ComponentId, BTreeMap<ImplementationLabel, Implementation>>,
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

/// A mapping whose entries keep file order so duplicate keys stay detectable.
struct Entries<T>(Vec<(String, T)>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Entries<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EntriesVisitor<T>(std::marker::PhantomData<T>);

        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for EntriesVisitor<T> {
            type Value = Entries<T>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a mapping")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry::<String, T>()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }

        deserializer.deserialize_map(EntriesVisitor(std::marker::PhantomData))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    components: Entries<ComponentFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentFile {
    implementations: Entries<ImplementationFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImplementationFile {
    language: LanguageFile,
    sources: PathBuf,
    tests: TestsFile,
    rust: Option<RustFile>,
    csharp: Option<CSharpFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TestsFile {
    projects: Option<Vec<PathBuf>>,
    command: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RustFile {
    #[serde(default)]
    runtime: RuntimeFile,
    executor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CSharpFile {
    framework: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum RuntimeFile {
    #[default]
    Auto,
    Arty,
    Smol,
    Tokio,
}

impl From<RuntimeFile> for Runtime {
    fn from(value: RuntimeFile) -> Self {
        match value {
            RuntimeFile::Auto => Self::Auto,
            RuntimeFile::Arty => Self::Arty,
            RuntimeFile::Smol => Self::Smol,
            RuntimeFile::Tokio => Self::Tokio,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum LanguageFile {
    Rust,
    CSharp,
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

/// A selected component implementation.
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_structs,
    reason = "resolved binding DTOs intentionally expose all selected implementation evidence"
)]
pub struct ResolvedImplementation {
    /// Absolute source binding path.
    pub binding_path: PathBuf,
    /// Component the implementation was selected for.
    pub component: ComponentId,
    /// Selected implementation label.
    pub label: ImplementationLabel,
    /// Selected implementation configuration.
    pub implementation: Implementation,
}

impl ResolvedImplementation {
    /// The implementation language.
    #[must_use]
    pub const fn language(&self) -> Language {
        self.implementation.language()
    }
}

impl Binding {
    /// Select one implementation of `component`.
    ///
    /// An omitted or empty `label` selects the component's only implementation.
    ///
    /// # Errors
    ///
    /// Returns an actionable error for an unknown component or label, or when
    /// `label` is omitted and the component has several implementations.
    pub fn resolve_implementation(&self, component: &str, label: Option<&str>) -> Result<ResolvedImplementation, Error> {
        let (component_id, implementations) = self.components.get_key_value(component).ok_or_else(|| {
            Error::message(
                ErrorKind::Binding,
                format!(
                    "component '{component}' not found in binding '{}'; available components: {}",
                    self.path.display(),
                    self.components.keys().map(ComponentId::as_str).collect::<Vec<_>>().join(", ")
                ),
            )
        })?;
        let labels = || {
            implementations
                .keys()
                .map(ImplementationLabel::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (label, implementation) = if let Some(label) = label {
            implementations.get_key_value(label).ok_or_else(|| {
                Error::message(
                    ErrorKind::Binding,
                    format!(
                        "implementation '{label}' not found for component '{component}'; available implementations: {}",
                        labels()
                    ),
                )
            })?
        } else {
            let mut entries = implementations.iter();
            match (entries.next(), entries.next()) {
                (Some(only), None) => only,
                _ => {
                    return Err(Error::message(
                        ErrorKind::Binding,
                        format!(
                            "component '{component}' has several implementations; select one by label: {}",
                            labels()
                        ),
                    ));
                }
            }
        };
        Ok(ResolvedImplementation {
            binding_path: self.path.clone(),
            component: component_id.clone(),
            label: label.clone(),
            implementation: implementation.clone(),
        })
    }
}

/// Load and validate a binding file.
///
/// # Errors
///
/// Rejects I/O/YAML errors, unknown fields, mismatched language blocks,
/// invalid test declarations, empty or duplicate identifiers, and empty paths.
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
/// Relative paths are resolved against `path` exactly as they are in
/// [`load_binding`], without reading or canonicalizing the filesystem.
///
/// # Errors
///
/// Rejects YAML errors, unknown fields, mismatched language blocks, invalid
/// test declarations, empty or duplicate identifiers, and empty paths.
pub fn parse_binding(path: impl AsRef<Path>, text: impl AsRef<str>) -> Result<Binding, Error> {
    let path = path.as_ref();
    let invalid = |message: String| {
        Error::message(
            ErrorKind::Binding,
            format!("binding '{}' not found or invalid: {message}", path.display()),
        )
    };
    let file: BindingFile = serde_yaml::from_str(text.as_ref()).map_err(|source| {
        Error::cause(
            ErrorKind::Binding,
            format!("binding '{}' not found or invalid: {source}", path.display()),
            source,
        )
    })?;
    if file.components.0.is_empty() {
        return Err(invalid("components must list at least one component".to_string()));
    }

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut components = BTreeMap::new();
    for (component, entry) in file.components.0 {
        if component.trim().is_empty() {
            return Err(invalid("component ids must not be empty".to_string()));
        }
        if entry.implementations.0.is_empty() {
            return Err(invalid(format!("component '{component}' must list at least one implementation")));
        }
        let mut implementations = BTreeMap::new();
        for (label, implementation) in entry.implementations.0 {
            if label.trim().is_empty() {
                return Err(invalid(format!("component '{component}' has an empty implementation label")));
            }
            let context = format!("implementation '{label}' of component '{component}'");
            let resolved = resolve_implementation_file(base, &context, implementation).map_err(|source| {
                Error::cause(
                    ErrorKind::Binding,
                    format!("binding '{}' not found or invalid: {source}", path.display()),
                    source,
                )
            })?;
            if implementations
                .insert(ImplementationLabel::from(label.as_str()), resolved)
                .is_some()
            {
                return Err(invalid(format!("component '{component}' repeats implementation label '{label}'")));
            }
        }
        if components.insert(ComponentId::from(component.as_str()), implementations).is_some() {
            return Err(invalid(format!("component '{component}' is listed more than once")));
        }
    }
    Ok(Binding {
        path: path.to_path_buf(),
        components,
    })
}

fn resolve_implementation_file(base: &Path, context: &str, file: ImplementationFile) -> Result<Implementation, Error> {
    let fail = |message: &str| Error::message(ErrorKind::Binding, format!("{context} {message}"));
    let options = match (file.language, file.rust, file.csharp) {
        (LanguageFile::Rust, rust, None) => {
            let (runtime, executor) = rust.map_or((Runtime::Auto, None), |block| (block.runtime.into(), block.executor));
            if runtime != Runtime::Auto && executor.is_some() {
                return Err(fail("must choose rust.runtime or rust.executor, not both"));
            }
            if executor.as_ref().is_some_and(|path| {
                !path.starts_with("crate::")
                    || path.split("::").skip(1).any(|segment| {
                        segment.is_empty()
                            || !segment
                                .strip_prefix("r#")
                                .unwrap_or(segment)
                                .chars()
                                .enumerate()
                                .all(|(index, character)| {
                                    character == '_' || character.is_ascii_alphabetic() || (index > 0 && character.is_ascii_digit())
                                })
                    })
            }) {
                return Err(fail(
                    "rust.executor must be a crate-rooted Rust function path such as crate::replay_executor",
                ));
            }
            LanguageOptions::Rust { runtime, executor }
        }
        (LanguageFile::CSharp, None, csharp) => LanguageOptions::CSharp {
            framework: csharp.and_then(|block| block.framework),
        },
        (LanguageFile::Rust, _, Some(_)) => return Err(fail("declares language rust but has a csharp block")),
        (LanguageFile::CSharp, Some(_), _) => return Err(fail("declares language csharp but has a rust block")),
    };
    let tests = match (file.tests.projects, file.tests.command) {
        (Some(projects), None) => Tests::Projects(
            projects
                .into_iter()
                .map(|project| {
                    if project.as_os_str().is_empty() {
                        project
                    } else {
                        normalize(base.join(project))
                    }
                })
                .collect(),
        ),
        (None, Some(command)) => Tests::Command(command),
        (Some(_), Some(_)) => return Err(fail("tests must declare exactly one of projects or command, not both")),
        (None, None) => return Err(fail("tests must declare exactly one of projects or command")),
    };
    let sources = if file.sources.as_os_str().is_empty() {
        file.sources
    } else {
        normalize(base.join(file.sources))
    };
    Implementation::new(sources, tests, options).map_err(|source| fail(&source.to_string()))
}

/// Load a binding and select one implementation of `component`.
///
/// # Errors
///
/// Returns binding parsing or implementation-selection errors.
pub fn resolve_implementation(path: impl AsRef<Path>, component: &str, label: Option<&str>) -> Result<ResolvedImplementation, Error> {
    load_binding(path)?.resolve_implementation(component, label)
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

    const VALID: &str = "components:
  example.orders:
    implementations:
      rust-main:
        language: rust
        sources: crates/orders
        tests:
          projects: [crates/orders/tests, ../shared/tests]
        rust:
          runtime: tokio
      cs-main:
        language: csharp
        sources: dotnet/Orders
        tests:
          command: dotnet test
        csharp:
          framework: net8.0
  example.users:
    implementations:
      only:
        language: rust
        sources: users
        tests:
          command: cargo test
";

    fn implementation(yaml: &str) -> String {
        format!(
            "components:\n  c:\n    implementations:\n      i:\n        language: rust\n        sources: s\n        tests:\n          command: t\n{yaml}"
        )
    }

    fn rejects(yaml: &str, expected: &str) {
        let error = parse_binding("specgate.yaml", yaml).unwrap_err();
        assert!(error.to_string().contains(expected), "expected '{expected}' in: {error}");
    }

    #[test]
    fn parses_valid_binding_with_resolved_paths() {
        let binding = parse_binding("base/specgate.yaml", VALID).unwrap();
        assert_eq!(binding.components.len(), 2);
        let rust = binding.resolve_implementation("example.orders", Some("rust-main")).unwrap();
        assert_eq!(rust.label, "rust-main");
        assert_eq!(rust.language(), Language::Rust);
        assert_eq!(rust.implementation.sources, PathBuf::from("base/crates/orders"));
        assert_eq!(
            rust.implementation.tests,
            Tests::Projects(vec![PathBuf::from("base/crates/orders/tests"), PathBuf::from("shared/tests")])
        );
        assert_eq!(
            rust.implementation.options,
            LanguageOptions::Rust {
                runtime: Runtime::Tokio,
                executor: None
            }
        );
        let csharp = binding.resolve_implementation("example.orders", Some("cs-main")).unwrap();
        assert_eq!(csharp.language(), Language::CSharp);
        assert_eq!(csharp.implementation.tests, Tests::Command("dotnet test".to_string()));
        assert_eq!(
            csharp.implementation.options,
            LanguageOptions::CSharp {
                framework: Some("net8.0".to_string())
            }
        );
    }

    #[test]
    fn rust_runtime_defaults_to_candidate_detection() {
        let binding = parse_binding("specgate.yaml", VALID).unwrap();
        let users = binding.resolve_implementation("example.users", None).unwrap();
        assert_eq!(
            users.implementation.options,
            LanguageOptions::Rust {
                runtime: Runtime::Auto,
                executor: None
            }
        );
    }

    #[test]
    fn supports_arty_and_custom_executor_bindings() {
        let binding = parse_binding("specgate.yaml", implementation("        rust:\n          runtime: arty\n")).unwrap();
        assert_eq!(
            binding.resolve_implementation("c", None).unwrap().implementation.options,
            LanguageOptions::Rust {
                runtime: Runtime::Arty,
                executor: None
            }
        );
        let binding = parse_binding(
            "specgate.yaml",
            implementation("        rust:\n          executor: crate::replay_executor\n"),
        )
        .unwrap();
        assert_eq!(
            binding.resolve_implementation("c", None).unwrap().implementation.options,
            LanguageOptions::Rust {
                runtime: Runtime::Auto,
                executor: Some("crate::replay_executor".to_string())
            }
        );
        rejects(
            &implementation("        rust:\n          runtime: arty\n          executor: crate::run\n"),
            "not both",
        );
        for path in ["run", "crate::", "crate::run()", "crate::run;panic!()", "crate::2run"] {
            rejects(
                &implementation(&format!("        rust:\n          executor: \"{path}\"\n")),
                "crate-rooted Rust function path",
            );
        }
    }

    #[test]
    fn rejects_missing_components() {
        rejects("{}\n", "missing field `components`");
        rejects("components: {}\n", "components must list at least one component");
        rejects("components:\n  c: {}\n", "missing field `implementations`");
        rejects(
            "components:\n  c:\n    implementations: {}\n",
            "must list at least one implementation",
        );
    }

    #[test]
    fn rejects_unknown_fields_at_every_level() {
        rejects(
            "version: 1\ncomponents:\n  c:\n    implementations: {}\n",
            "unknown field `version`",
        );
        rejects(
            "components:\n  c:\n    surprise: true\n    implementations: {}\n",
            "unknown field `surprise`",
        );
        rejects(&implementation("        surprise: true\n"), "unknown field `surprise`");
        rejects(
            &implementation("        rust:\n          surprise: true\n"),
            "unknown field `surprise`",
        );
    }

    #[test]
    fn rejects_wrong_language_block() {
        rejects(
            &implementation("        csharp:\n          framework: net8.0\n"),
            "language rust but has a csharp block",
        );
        let csharp = "components:\n  c:\n    implementations:\n      i:\n        language: csharp\n        sources: s\n        tests:\n          command: t\n        rust:\n          runtime: tokio\n";
        rejects(csharp, "language csharp but has a rust block");
    }

    #[test]
    fn rejects_invalid_tests_declarations() {
        let with_tests = |tests: &str| {
            format!(
                "components:\n  c:\n    implementations:\n      i:\n        language: rust\n        sources: s\n        tests:\n{tests}"
            )
        };
        rejects(&with_tests("          projects: [p]\n          command: t\n"), "not both");
        rejects(&with_tests("          {}\n"), "exactly one of projects or command");
        rejects(&with_tests("          projects: []\n"), "at least one path");
        rejects(&with_tests("          projects: ['']\n"), "empty path");
        rejects(&with_tests("          command: '  '\n"), "must not be blank");
    }

    #[test]
    fn rejects_empty_and_duplicate_identifiers() {
        rejects("components:\n  '':\n    implementations: {}\n", "component ids must not be empty");
        let labelled = |label: &str| {
            format!(
                "components:\n  c:\n    implementations:\n      {label}:\n        language: rust\n        sources: s\n        tests:\n          command: t\n"
            )
        };
        rejects(&labelled("''"), "empty implementation label");
        let duplicate_label = format!(
            "{}      i:\n        language: rust\n        sources: s\n        tests:\n          command: t\n",
            labelled("i")
        );
        rejects(&duplicate_label, "repeats implementation label 'i'");
        let duplicate_component = format!(
            "{}  c:\n    implementations:\n      i:\n        language: rust\n        sources: s\n        tests:\n          command: t\n",
            labelled("i")
        );
        rejects(&duplicate_component, "listed more than once");
    }

    #[test]
    fn rejects_empty_sources() {
        rejects(
            &implementation("").replace("sources: s", "sources: ''"),
            "sources must not be empty",
        );
    }

    #[test]
    fn rejects_old_format_files() {
        rejects(
            "language: rust\ntargets:\n  default:\n    package_root: .\n",
            "unknown field `language`",
        );
    }

    #[test]
    fn selection_auto_selects_a_sole_implementation() {
        let binding = parse_binding("specgate.yaml", VALID).unwrap();
        let resolved = binding.resolve_implementation("example.users", None).unwrap();
        assert_eq!(resolved.label, "only");
        assert_eq!(resolved.component, "example.users");
    }

    #[test]
    fn selection_reports_ambiguity_and_unknown_choices() {
        let binding = parse_binding("specgate.yaml", VALID).unwrap();
        let ambiguous = binding.resolve_implementation("example.orders", None).unwrap_err();
        assert!(ambiguous.to_string().contains("several implementations"));
        assert!(ambiguous.to_string().contains("cs-main, rust-main"));
        let unknown_label = binding.resolve_implementation("example.orders", Some("nope")).unwrap_err();
        assert!(unknown_label.to_string().contains("implementation 'nope' not found"));
        assert!(unknown_label.to_string().contains("cs-main, rust-main"));
        let empty_label = binding.resolve_implementation("example.orders", Some("")).unwrap_err();
        assert!(empty_label.to_string().contains("implementation '' not found"));
        let unknown_component = binding.resolve_implementation("missing", None).unwrap_err();
        assert!(unknown_component.to_string().contains("component 'missing' not found"));
        assert!(unknown_component.to_string().contains("example.orders, example.users"));
    }

    #[test]
    fn content_and_path_loading_have_identical_results() {
        let fixture = binding_file(VALID);
        let loaded = load_binding(&fixture.path).unwrap();
        let parsed = parse_binding(&loaded.path, VALID).unwrap();
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
        let filesystem = Filesystem {
            inner: FilesystemKind::Fake(FakeFs {
                canonicalize: Ok(canonical.clone()),
                read_to_string: Ok(VALID.to_string()),
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
