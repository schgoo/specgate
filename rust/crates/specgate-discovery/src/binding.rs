//! Strict CTSC target-binding parsing and path resolution.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Async runtime selected by a Rust target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Runtime {
    #[default]
    Smol,
    Tokio,
}

/// Structured command output declaration retained by CTSC target bindings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetOutputs {
    pub file: Option<String>,
    pub stdout: Option<OutputFormat>,
}

/// Supported structured stdout formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Json,
    Yaml,
}

/// One unresolved target exactly as represented in YAML.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetFile {
    package_root: String,
    #[serde(default)]
    runtime: Runtime,
    framework: Option<String>,
    command: Option<String>,
    outputs: Option<TargetOutputs>,
}

/// One resolved implementation target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub package_root: PathBuf,
    pub runtime: Runtime,
    pub framework: Option<String>,
    pub command: Option<String>,
    pub outputs: Option<TargetOutputs>,
}

/// A parsed binding with every path resolved relative to the binding file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub path: PathBuf,
    pub language: String,
    pub targets: BTreeMap<String, Target>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    language: String,
    targets: BTreeMap<String, TargetFile>,
}

/// A selected binding target.
#[derive(Debug, Clone)]
pub struct ResolvedTarget {
    pub binding_path: PathBuf,
    pub name: String,
    pub language: String,
    pub target: Target,
}

impl Binding {
    /// Select a named target, or `default` and then the first sorted target.
    ///
    /// # Errors
    ///
    /// Returns an actionable error for an unknown target.
    pub fn resolve_target(&self, requested: Option<&str>) -> Result<ResolvedTarget, String> {
        let (name, target) = match requested.filter(|name| !name.is_empty()) {
            Some(name) => self.targets.get_key_value(name).ok_or_else(|| {
                format!(
                    "target '{name}' not found in binding; available targets: {}",
                    self.targets.keys().cloned().collect::<Vec<_>>().join(", ")
                )
            })?,
            None => self
                .targets
                .get_key_value("default")
                .or_else(|| self.targets.first_key_value())
                .ok_or_else(|| format!("binding '{}' contains no targets", self.path.display()))?,
        };
        Ok(ResolvedTarget {
            binding_path: self.path.clone(),
            name: name.clone(),
            language: self.language.clone(),
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
pub fn load_binding(path: &Path) -> Result<Binding, String> {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let text = std::fs::read_to_string(&path).map_err(|error| format!("binding '{}' not found or invalid: {error}", path.display()))?;
    let file: BindingFile =
        serde_yaml::from_str(&text).map_err(|error| format!("binding '{}' not found or invalid: {error}", path.display()))?;
    if !matches!(file.language.as_str(), "rust" | "csharp") {
        return Err(format!(
            "binding '{}' uses unsupported language '{}'",
            path.display(),
            file.language
        ));
    }
    if file.targets.is_empty() {
        return Err(format!("binding '{}' contains no targets", path.display()));
    }

    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let mut targets = BTreeMap::new();
    for (name, target) in file.targets {
        if name.trim().is_empty() {
            return Err(format!("binding '{}' contains an empty target name", path.display()));
        }
        if target.package_root.trim().is_empty() {
            return Err(format!("binding target '{name}' has an empty package_root"));
        }
        let package_root = normalize(&base.join(&target.package_root));
        targets.insert(
            name,
            Target {
                package_root,
                runtime: target.runtime,
                framework: target.framework,
                command: target.command,
                outputs: target.outputs,
            },
        );
    }
    Ok(Binding {
        path,
        language: file.language,
        targets,
    })
}

/// Load a binding and select one target using the shared defaulting rules.
///
/// # Errors
///
/// Returns binding parsing or target-selection errors.
pub fn resolve_binding_target(path: &str, requested: Option<&str>) -> Result<ResolvedTarget, String> {
    load_binding(Path::new(path))?.resolve_target(requested)
}

fn normalize(path: &Path) -> PathBuf {
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
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static ID: AtomicU64 = AtomicU64::new(0);

    struct BindingFixture {
        _cache: crate::support::InvocationCache,
        path: PathBuf,
    }

    fn binding_file(yaml: &str) -> BindingFixture {
        let cache = crate::support::InvocationCache::create("binding-tests", "binding", ID.fetch_add(1, Ordering::Relaxed)).unwrap();
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
}
