//! Runtime cache, Cargo source resolution, and generated manifest support.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Invocation cache directory removed automatically when dropped.
#[derive(Debug)]
pub struct InvocationCache {
    path: PathBuf,
}

impl InvocationCache {
    /// Create one invocation-unique operating-system cache directory.
    ///
    /// # Errors
    ///
    /// Returns an error when no cache root is available or creation fails.
    pub fn create(scope: &str, label: &str, invocation: u64) -> Result<Self, String> {
        let root = cache_root()?;
        let path = root
            .join("specgate")
            .join(scope)
            .join(format!("{}-{}-{invocation}", sanitize(label), std::process::id()));
        std::fs::create_dir_all(&path).map_err(|error| format!("failed to create runtime cache directory {}: {error}", path.display()))?;
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for InvocationCache {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl Drop for InvocationCache {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Exact Cargo source for the runtime package used by a candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoPackageSource {
    pub package: String,
    pub version: String,
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
}

/// Candidate package identity plus its exact resolved runtime source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateCargoContext {
    pub package: String,
    pub version: String,
    pub path: PathBuf,
    pub runtime: CargoPackageSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeOverride {
    Path { package_id: String },
    Version(String),
}

/// One dependency in a generated Cargo manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestDependency {
    pub package: String,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    pub registry: Option<String>,
}

impl ManifestDependency {
    #[must_use]
    pub fn from_source(source: &CargoPackageSource) -> Self {
        Self {
            package: source.package.clone(),
            version: Some(format!("={}", source.version)),
            path: source.path.clone(),
            registry: source.registry.clone(),
        }
    }

    #[must_use]
    pub fn local(package: impl Into<String>, version: impl Into<String>, path: PathBuf) -> Self {
        Self {
            package: package.into(),
            version: Some(format!("={}", version.into())),
            path: Some(path),
            registry: None,
        }
    }
}

/// Generated Cargo files for an isolated runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerCargo {
    pub manifest: String,
    pub config: Option<String>,
}

/// Resolve the candidate and exact `specgate-runtime` source through
/// `cargo metadata` rooted at the candidate package.
///
/// Explicit overrides are available through `SPECGATE_RUNTIME_PATH` or
/// `SPECGATE_RUNTIME_VERSION`; setting both is an error. Overrides must select
/// the same Cargo `PackageId` already resolved by the candidate. No caller-CWD
/// source discovery is performed.
///
/// # Errors
///
/// Returns metadata, graph, ambiguity, unsupported-source, or override errors.
pub fn candidate_cargo_context(package_root: &Path) -> Result<CandidateCargoContext, String> {
    let runtime_override = runtime_override()?;
    candidate_cargo_context_with_override(package_root, runtime_override.as_ref())
}

fn candidate_cargo_context_with_override(
    package_root: &Path,
    runtime_override: Option<&RuntimeOverride>,
) -> Result<CandidateCargoContext, String> {
    let manifest =
        std::fs::canonicalize(package_root.join("Cargo.toml")).map_err(|error| format!("cannot resolve candidate Cargo.toml: {error}"))?;
    let package_root = manifest.parent().ok_or_else(|| "candidate Cargo.toml has no parent".to_string())?;
    let metadata = cargo_metadata(&manifest, package_root)?;
    let candidate = metadata
        .packages
        .iter()
        .find(|package| same_path(&package.manifest_path, &manifest))
        .ok_or_else(|| format!("cargo metadata did not return candidate package {}", manifest.display()))?;
    let runtime_package = resolve_runtime_package(&metadata, &candidate.id)?;
    let runtime = match runtime_override {
        Some(runtime_override) => runtime_source_for_override(runtime_package, runtime_override)?,
        None => package_source(runtime_package)?,
    };
    Ok(CandidateCargoContext {
        package: candidate.name.clone(),
        version: candidate.version.clone(),
        path: package_root.to_path_buf(),
        runtime,
    })
}

/// Serialize a standalone generated Cargo manifest and any registry config
/// needed to preserve dependency source identity.
///
/// # Errors
///
/// Returns an error for invalid sources, non-UTF-8 paths, alias collisions, or
/// TOML serialization failure.
pub fn runner_cargo(package_name: &str, dependencies: BTreeMap<String, ManifestDependency>) -> Result<RunnerCargo, String> {
    let mut registries = BTreeMap::new();
    let dependencies = dependencies
        .into_iter()
        .map(|(alias, dependency)| {
            let path = dependency.path.as_deref().map(cargo_path).transpose()?;
            if path.is_some() && dependency.registry.is_some() {
                return Err(format!(
                    "Cargo dependency '{}' cannot declare both path and registry sources",
                    dependency.package
                ));
            }
            let registry = dependency
                .registry
                .as_deref()
                .filter(|source| !is_crates_io_registry(source))
                .map(|source| {
                    let registry_alias = registry_alias(source);
                    let index = registry_index(source)?.to_string();
                    if let Some(existing) = registries.insert(registry_alias.clone(), SerializableRegistry { index: index.clone() })
                        && existing.index != index
                    {
                        return Err(format!("generated Cargo registry alias collision for source '{source}'"));
                    }
                    Ok(registry_alias)
                })
                .transpose()?;
            Ok((
                alias,
                SerializableDependency {
                    package: dependency.package,
                    version: dependency.version,
                    path,
                    registry,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let mut manifest = toml::to_string(&GeneratedManifest {
        package: GeneratedPackage {
            name: package_name,
            version: "0.0.0",
            edition: "2024",
            publish: false,
        },
        dependencies,
    })
    .map_err(|error| format!("failed to serialize generated Cargo manifest: {error}"))?;
    manifest.push_str("\n[workspace]\n");
    let config = if registries.is_empty() {
        None
    } else {
        Some(
            toml::to_string(&GeneratedCargoConfig { registries })
                .map_err(|error| format!("failed to serialize generated Cargo registry config: {error}"))?,
        )
    };
    Ok(RunnerCargo { manifest, config })
}

#[derive(Serialize)]
struct GeneratedManifest<'a> {
    package: GeneratedPackage<'a>,
    dependencies: BTreeMap<String, SerializableDependency>,
}

#[derive(Serialize)]
struct GeneratedPackage<'a> {
    name: &'a str,
    version: &'static str,
    edition: &'static str,
    publish: bool,
}

#[derive(Serialize)]
struct SerializableDependency {
    package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    registry: Option<String>,
}

#[derive(Serialize)]
struct GeneratedCargoConfig {
    registries: BTreeMap<String, SerializableRegistry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SerializableRegistry {
    index: String,
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
    resolve: Option<CargoResolve>,
}

#[derive(Deserialize)]
struct CargoMetadataPackage {
    id: String,
    name: String,
    version: String,
    manifest_path: PathBuf,
    source: Option<String>,
}

#[derive(Deserialize)]
struct CargoResolve {
    nodes: Vec<CargoNode>,
}

#[derive(Deserialize)]
struct CargoNode {
    id: String,
    #[serde(default)]
    deps: Vec<CargoNodeDependency>,
}

#[derive(Deserialize)]
struct CargoNodeDependency {
    pkg: String,
}

fn cargo_metadata(manifest: &Path, package_root: &Path) -> Result<CargoMetadata, String> {
    let output = Command::new(cargo_bin())
        .arg("metadata")
        .arg("--format-version")
        .arg("1")
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(package_root)
        .output()
        .map_err(|error| format!("failed to invoke cargo metadata: {error}"))?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("cargo metadata output was malformed: {error}"))
}

fn resolve_runtime_package<'a>(metadata: &'a CargoMetadata, candidate_id: &str) -> Result<&'a CargoMetadataPackage, String> {
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| "cargo metadata returned no resolved dependency graph".to_string())?;
    let packages = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect::<BTreeMap<_, _>>();
    let nodes = resolve
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    let candidate = nodes
        .get(candidate_id)
        .ok_or_else(|| "candidate package is absent from cargo metadata resolve graph".to_string())?;

    let direct_specgate = candidate
        .deps
        .iter()
        .filter_map(|dependency| packages.get(dependency.pkg.as_str()).copied())
        .find(|package| package.name == "specgate");
    let start = direct_specgate.map_or(candidate_id, |package| package.id.as_str());

    let mut queue = VecDeque::from([start]);
    let mut visited = BTreeSet::new();
    let mut runtimes = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(package) = packages.get(id)
            && package.name == "specgate-runtime"
        {
            runtimes.insert(id);
            continue;
        }
        if let Some(node) = nodes.get(id) {
            queue.extend(node.deps.iter().map(|dependency| dependency.pkg.as_str()));
        }
    }
    let runtime_id = match runtimes.into_iter().collect::<Vec<_>>().as_slice() {
        [] => return Err("candidate dependency graph contains no specgate-runtime package".to_string()),
        [runtime] => *runtime,
        runtimes => {
            return Err(format!(
                "candidate dependency graph contains ambiguous specgate-runtime packages: {}",
                runtimes.join(", ")
            ));
        }
    };
    packages
        .get(runtime_id)
        .copied()
        .ok_or_else(|| "resolved specgate-runtime package metadata is missing".to_string())
}

fn runtime_override() -> Result<Option<RuntimeOverride>, String> {
    let path = nonempty_env_path("SPECGATE_RUNTIME_PATH");
    let version = std::env::var("SPECGATE_RUNTIME_VERSION").ok().filter(|value| !value.is_empty());
    match (path, version) {
        (Some(_), Some(_)) => Err("set only one of SPECGATE_RUNTIME_PATH or SPECGATE_RUNTIME_VERSION".to_string()),
        (Some(path), None) => runtime_path_override(&path).map(Some),
        (None, Some(version)) => Ok(Some(RuntimeOverride::Version(version))),
        (None, None) => Ok(None),
    }
}

fn runtime_path_override(path: &Path) -> Result<RuntimeOverride, String> {
    let manifest = std::fs::canonicalize(path.join("Cargo.toml")).map_err(|error| format!("invalid SPECGATE_RUNTIME_PATH: {error}"))?;
    let package_root = manifest
        .parent()
        .ok_or_else(|| "SPECGATE_RUNTIME_PATH Cargo.toml has no parent".to_string())?;
    let metadata = cargo_metadata(&manifest, package_root)?;
    let package = metadata
        .packages
        .iter()
        .find(|package| same_path(&package.manifest_path, &manifest) && package.name == "specgate-runtime")
        .ok_or_else(|| "SPECGATE_RUNTIME_PATH is not a specgate-runtime package".to_string())?;
    Ok(RuntimeOverride::Path {
        package_id: package.id.clone(),
    })
}

fn runtime_source_for_override(
    candidate_runtime: &CargoMetadataPackage,
    runtime_override: &RuntimeOverride,
) -> Result<CargoPackageSource, String> {
    match runtime_override {
        RuntimeOverride::Path { package_id } if package_id == &candidate_runtime.id => package_source(candidate_runtime),
        RuntimeOverride::Path { package_id } => Err(format!(
            "SPECGATE_RUNTIME_PATH resolves to Cargo PackageId '{package_id}', which is a different Cargo PackageId from the \
             candidate's specgate-runtime '{}'; using both would create distinct runtime/linkme/capture state; remove the override or \
             align the candidate dependency",
            candidate_runtime.id
        )),
        RuntimeOverride::Version(version) if version != &candidate_runtime.version => Err(format!(
            "SPECGATE_RUNTIME_VERSION={version} does not match: the candidate resolves specgate-runtime version {} as Cargo PackageId \
             '{}'; remove the override or align the candidate dependency",
            candidate_runtime.version, candidate_runtime.id
        )),
        RuntimeOverride::Version(version) => match candidate_runtime.source.as_deref() {
            Some(source) if is_crates_io_registry(source) => package_source(candidate_runtime),
            _ => Err(format!(
                "SPECGATE_RUNTIME_VERSION={version} selects crates.io, but the candidate resolves specgate-runtime as Cargo PackageId \
                 '{}'; using both would create distinct runtime/linkme/capture state; remove the override or use \
                 SPECGATE_RUNTIME_PATH only when it resolves to that exact PackageId",
                candidate_runtime.id
            )),
        },
    }
}

fn package_source(package: &CargoMetadataPackage) -> Result<CargoPackageSource, String> {
    let resolved_path = package
        .manifest_path
        .parent()
        .ok_or_else(|| format!("resolved package Cargo.toml has no parent: {}", package.manifest_path.display()))?
        .to_path_buf();
    let (path, registry) = match package.source.as_deref() {
        None => (Some(resolved_path), None),
        Some(source) if registry_index(source).is_ok() => (None, Some(source.to_string())),
        Some(source) => {
            return Err(format!(
                "unsupported specgate-runtime Cargo source '{source}' for PackageId '{}'; generated runners cannot preserve this \
                 identity without a graph-wide Cargo source replacement",
                package.id
            ));
        }
    };
    Ok(CargoPackageSource {
        package: package.name.clone(),
        version: package.version.clone(),
        path,
        registry,
    })
}

fn registry_index(source: &str) -> Result<&str, String> {
    if let Some(index) = source.strip_prefix("registry+") {
        if index.is_empty() {
            return Err("registry source URL is empty".to_string());
        }
        return Ok(index);
    }
    if source.starts_with("sparse+") && source.len() > "sparse+".len() {
        return Ok(source);
    }
    Err(format!("unsupported Cargo registry source '{source}'"))
}

fn is_crates_io_registry(source: &str) -> bool {
    matches!(
        source,
        "registry+https://github.com/rust-lang/crates.io-index"
            | "registry+https://index.crates.io"
            | "registry+https://index.crates.io/"
            | "sparse+https://index.crates.io/"
            | "registry+sparse+https://index.crates.io/"
    )
}

fn registry_alias(source: &str) -> String {
    let hash = source.as_bytes().iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    });
    format!("specgate-source-{hash:016x}")
}

fn cache_root() -> Result<PathBuf, String> {
    if let Some(path) = nonempty_env_path("SPECGATE_CACHE_DIR") {
        return Ok(path);
    }
    #[cfg(windows)]
    if let Some(path) = nonempty_env_path("LOCALAPPDATA") {
        return Ok(path.join("SpecGate").join("Cache"));
    }
    if let Some(path) = nonempty_env_path("XDG_CACHE_HOME") {
        return Ok(path);
    }
    if let Some(path) = nonempty_env_path("HOME") {
        return Ok(path.join(".cache"));
    }
    Err("no operating-system cache directory is available; set SPECGATE_CACHE_DIR".to_string())
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn same_path(left: &Path, right: &Path) -> bool {
    std::fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == std::fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

fn cargo_path(path: &Path) -> Result<String, String> {
    let value = path
        .to_str()
        .ok_or_else(|| format!("Cargo dependency path is not UTF-8: {}", path.display()))?;
    #[cfg(windows)]
    {
        let value = value.strip_prefix(r"\\?\").unwrap_or(value);
        Ok(value.replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Ok(value.to_string())
    }
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn sanitize(value: &str) -> String {
    value
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
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::thread::JoinHandle;
    use std::time::Duration;

    static TEST_ID: AtomicU64 = AtomicU64::new(0);
    const REGISTRY_RUNTIME_VERSION: &str = "99.0.0";

    #[test]
    fn manifest_serialization_escapes_all_dynamic_toml_strings() {
        let mut dependencies = BTreeMap::new();
        dependencies.insert(
            "candidate alias".to_string(),
            ManifestDependency {
                package: "candidate\"quoted".to_string(),
                version: Some("=0.6.0\\metadata".to_string()),
                path: Some(PathBuf::from("path\"quoted\\segment")),
                registry: None,
            },
        );
        let cargo = runner_cargo("runner\"quoted", dependencies).unwrap();
        let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
        assert_eq!(parsed["package"]["name"].as_str(), Some("runner\"quoted"));
        assert_eq!(
            parsed["dependencies"]["candidate alias"]["package"].as_str(),
            Some("candidate\"quoted")
        );
        assert_eq!(cargo.config, None);
        #[cfg(windows)]
        assert_eq!(
            parsed["dependencies"]["candidate alias"]["path"].as_str(),
            Some("path\"quoted/segment")
        );
        #[cfg(not(windows))]
        assert_eq!(
            parsed["dependencies"]["candidate alias"]["path"].as_str(),
            Some("path\"quoted\\segment")
        );
    }

    #[test]
    fn candidate_metadata_ignores_misleading_caller_checkout() {
        let cache = InvocationCache::create("tests", "metadata-source", TEST_ID.fetch_add(1, Ordering::Relaxed)).unwrap();
        let runtime = cache.path().join("runtime");
        let candidate = cache.path().join("candidate");
        std::fs::create_dir_all(runtime.join("src")).unwrap();
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::write(
            runtime.join("Cargo.toml"),
            "[package]\nname=\"specgate-runtime\"\nversion=\"0.6.0\"\nedition=\"2024\"\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(runtime.join("src/lib.rs"), "").unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={path=\"../runtime\"}\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(candidate.join("src/lib.rs"), "").unwrap();

        let context = candidate_cargo_context(&candidate).unwrap();
        assert_eq!(context.runtime.path.as_deref(), Some(runtime.as_path()));
        assert!(
            !context
                .runtime
                .path
                .unwrap()
                .starts_with(std::env::current_dir().unwrap().join("rust").join("crates"))
        );
    }

    #[test]
    fn mismatched_runtime_path_override_is_rejected() {
        let project = TestProject::create("mismatched-runtime-path");
        let candidate_runtime = project.path().join("candidate-runtime");
        let override_runtime = project.path().join("override-runtime");
        let candidate = project.path().join("candidate");
        create_path_runtime(&candidate_runtime, "0.6.0");
        create_path_runtime(&override_runtime, "0.6.0");
        create_path_candidate(&candidate, &candidate_runtime);

        let override_ = runtime_path_override(&override_runtime).unwrap();
        let error = candidate_cargo_context_with_override(&candidate, Some(&override_)).unwrap_err();

        assert!(error.contains("SPECGATE_RUNTIME_PATH"));
        assert!(error.contains("different Cargo PackageId"));
        assert!(error.contains("remove the override or align the candidate dependency"));
    }

    #[test]
    fn mismatched_runtime_version_override_is_rejected() {
        let project = TestProject::create("mismatched-runtime-version");
        let runtime = project.path().join("runtime");
        let candidate = project.path().join("candidate");
        create_path_runtime(&runtime, "0.6.0");
        create_path_candidate(&candidate, &runtime);

        let error = candidate_cargo_context_with_override(&candidate, Some(&RuntimeOverride::Version("0.7.0".to_string()))).unwrap_err();

        assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.7.0"));
        assert!(error.contains("candidate resolves specgate-runtime version 0.6.0"));
        assert!(error.contains("remove the override or align the candidate dependency"));
    }

    #[test]
    fn matching_version_override_rejects_a_path_package_id() {
        let project = TestProject::create("matching-version-path-identity");
        let runtime = project.path().join("runtime");
        let candidate = project.path().join("candidate");
        create_path_runtime(&runtime, "0.6.0");
        create_path_candidate(&candidate, &runtime);

        let error = candidate_cargo_context_with_override(&candidate, Some(&RuntimeOverride::Version("0.6.0".to_string()))).unwrap_err();

        assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.6.0 selects crates.io"));
        assert!(error.contains("distinct runtime/linkme/capture state"));
        assert!(error.contains("remove the override"));
    }

    #[test]
    fn override_validation_does_not_eagerly_resolve_an_unsupported_source() {
        let runtime = CargoMetadataPackage {
            id: "git+https://example.invalid/specgate#specgate-runtime@0.6.0".to_string(),
            name: "specgate-runtime".to_string(),
            version: "0.6.0".to_string(),
            manifest_path: PathBuf::from("checkout/specgate-runtime/Cargo.toml"),
            source: Some("git+https://example.invalid/specgate".to_string()),
        };

        let error = runtime_source_for_override(&runtime, &RuntimeOverride::Version("0.6.0".to_string())).unwrap_err();

        assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.6.0 selects crates.io"));
        assert!(!error.contains("unsupported specgate-runtime Cargo source"));
    }

    #[test]
    fn matching_runtime_path_override_keeps_runner_capture_state_shared() {
        let project = TestProject::create("matching-runtime-path");
        let runtime = project.path().join("runtime");
        let candidate = project.path().join("candidate");
        let runner = project.path().join("runner");
        create_path_runtime(&runtime, "0.6.0");
        create_path_candidate(&candidate, &runtime);

        let override_ = runtime_path_override(&runtime).unwrap();
        let context = candidate_cargo_context_with_override(&candidate, Some(&override_)).unwrap();

        assert_eq!(context.runtime.path.as_deref(), Some(runtime.as_path()));
        run_capture_state_runner(&candidate, &runner, &context);
    }

    #[test]
    fn registry_packages_preserve_their_registry_source() {
        for source in [
            "registry+https://github.com/rust-lang/crates.io-index",
            "registry+https://registry.example.invalid/index",
            "registry+sparse+https://registry.example.invalid/index/",
            "sparse+https://registry.example.invalid/index/",
        ] {
            let package = CargoMetadataPackage {
                id: format!("registry-package#{source}"),
                name: "specgate-runtime".to_string(),
                version: "0.6.0".to_string(),
                manifest_path: PathBuf::from("cargo-home")
                    .join("registry")
                    .join("src")
                    .join("resolved-source")
                    .join("specgate-runtime-0.6.0")
                    .join("Cargo.toml"),
                source: Some(source.to_string()),
            };
            let resolved = package_source(&package).unwrap();
            assert_eq!(resolved.path, None);
            assert_eq!(resolved.registry.as_deref(), Some(source));
        }
    }

    #[test]
    fn alternate_registry_dependencies_generate_deterministic_alias_config() {
        for (source, expected_index) in [
            (
                "registry+https://registry.example.invalid/git-index",
                "https://registry.example.invalid/git-index",
            ),
            (
                "registry+sparse+https://registry.example.invalid/sparse-index/",
                "sparse+https://registry.example.invalid/sparse-index/",
            ),
            (
                "sparse+https://registry.example.invalid/direct-sparse-index/",
                "sparse+https://registry.example.invalid/direct-sparse-index/",
            ),
        ] {
            let dependency = ManifestDependency::from_source(&CargoPackageSource {
                package: "specgate-runtime".to_string(),
                version: "0.6.0".to_string(),
                path: None,
                registry: Some(source.to_string()),
            });
            let first = runner_cargo(
                "alternate-registry-runner",
                BTreeMap::from([("specgate_runtime".to_string(), dependency.clone())]),
            )
            .unwrap();
            let second = runner_cargo(
                "alternate-registry-runner",
                BTreeMap::from([("specgate_runtime".to_string(), dependency)]),
            )
            .unwrap();
            assert_eq!(first, second);

            let manifest: toml::Value = toml::from_str(&first.manifest).unwrap();
            let alias = manifest["dependencies"]["specgate_runtime"]["registry"].as_str().unwrap();
            assert_eq!(manifest["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
            assert!(manifest["dependencies"]["specgate_runtime"].get("path").is_none());
            let config: toml::Value = toml::from_str(first.config.as_deref().unwrap()).unwrap();
            assert_eq!(config["registries"][alias]["index"].as_str(), Some(expected_index));
        }
    }

    #[test]
    fn crates_io_dependencies_use_the_exact_default_registry_version() {
        for source in [
            "registry+https://github.com/rust-lang/crates.io-index",
            "sparse+https://index.crates.io/",
        ] {
            let cargo = runner_cargo(
                "crates-io-runner",
                BTreeMap::from([(
                    "specgate_runtime".to_string(),
                    ManifestDependency::from_source(&CargoPackageSource {
                        package: "specgate-runtime".to_string(),
                        version: "0.6.0".to_string(),
                        path: None,
                        registry: Some(source.to_string()),
                    }),
                )]),
            )
            .unwrap();
            let manifest: toml::Value = toml::from_str(&cargo.manifest).unwrap();
            assert_eq!(manifest["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
            assert!(manifest["dependencies"]["specgate_runtime"].get("path").is_none());
            assert!(manifest["dependencies"]["specgate_runtime"].get("registry").is_none());
            assert_eq!(cargo.config, None);
        }
    }

    #[test]
    fn registry_source_replacement_keeps_runner_capture_state_shared() {
        let project = TestProject::create("registry-source-identity");
        let registry = project.path().join("registry");
        let runtime = project.path().join("runtime");
        let candidate = project.path().join("candidate");
        let runner = project.path().join("runner");
        let (index_entry, archive) = package_registry_runtime(&runtime);
        create_local_registry(&registry, &index_entry, &archive);
        create_registry_candidate(&candidate, &registry);

        let context =
            candidate_cargo_context_with_override(&candidate, Some(&RuntimeOverride::Version(REGISTRY_RUNTIME_VERSION.to_string())))
                .unwrap();
        assert_eq!(context.runtime.path, None);
        assert_eq!(
            context.runtime.registry.as_deref(),
            Some("registry+https://github.com/rust-lang/crates.io-index")
        );
        run_capture_state_runner(&candidate, &runner, &context);
    }

    #[test]
    fn alternate_sparse_registry_keeps_runner_capture_state_shared() {
        let project = TestProject::create("alternate-registry-identity");
        let runtime = project.path().join("runtime");
        let candidate = project.path().join("candidate");
        let runner = project.path().join("runner");
        let (index_entry, archive) = package_registry_runtime(&runtime);
        let registry = SparseRegistry::start(index_entry, archive);
        create_alternate_registry_candidate(&candidate, registry.index());

        let context = candidate_cargo_context(&candidate).unwrap();
        assert_eq!(context.runtime.path, None);
        assert_eq!(context.runtime.registry.as_deref(), Some(registry.index()));
        run_capture_state_runner(&candidate, &runner, &context);
    }

    fn run_capture_state_runner(candidate: &Path, runner: &Path, context: &CandidateCargoContext) {
        std::fs::create_dir_all(runner.join("src")).unwrap();
        let cargo = runner_cargo(
            "registry-identity-runner",
            BTreeMap::from([
                (
                    "candidate".to_string(),
                    ManifestDependency::local(context.package.clone(), context.version.clone(), context.path.clone()),
                ),
                ("specgate_runtime".to_string(), ManifestDependency::from_source(&context.runtime)),
            ]),
        )
        .unwrap();
        std::fs::write(runner.join("Cargo.toml"), cargo.manifest).unwrap();
        let config = cargo.config.map(|config| {
            let path = runner.join("registry-config.toml");
            std::fs::write(&path, config).unwrap();
            path
        });
        std::fs::write(
            runner.join("src").join("main.rs"),
            "fn main() {\n    candidate::start_capture();\n    assert!(specgate_runtime::native_capture_is_active());\n}\n",
        )
        .unwrap();

        let mut command = Command::new(cargo_bin());
        command.arg("run").arg("--quiet");
        if let Some(config) = config {
            command.arg("--config").arg(config);
        }
        let output = command
            .arg("--manifest-path")
            .arg(runner.join("Cargo.toml"))
            .current_dir(candidate)
            .env_remove("CARGO")
            .env_remove("CARGO_MANIFEST_DIR")
            .env("CARGO_TARGET_DIR", runner.join("target"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "runner and candidate used distinct runtime capture state:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    struct TestProject {
        path: PathBuf,
    }

    impl TestProject {
        fn create(label: &str) -> Self {
            let rust_root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap();
            let path = rust_root.join("target").join("specgate-discovery-tests").join(format!(
                "{label}-{}-{}",
                std::process::id(),
                TEST_ID.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn package_registry_runtime(runtime: &Path) -> (String, Vec<u8>) {
        std::fs::create_dir_all(runtime.join("src")).unwrap();
        std::fs::write(
            runtime.join("Cargo.toml"),
            format!(
                "[package]\nname=\"specgate-runtime\"\nversion=\"{REGISTRY_RUNTIME_VERSION}\"\nedition=\"2024\"\ndescription=\"registry identity fixture\"\nlicense=\"MIT\"\n[workspace]\n"
            ),
        )
        .unwrap();
        std::fs::write(
            runtime.join("src").join("lib.rs"),
            "use std::sync::atomic::{AtomicBool, Ordering};\nstatic CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);\npub fn start_native_capture() { CAPTURE_ACTIVE.store(true, Ordering::SeqCst); }\npub fn native_capture_is_active() -> bool { CAPTURE_ACTIVE.load(Ordering::SeqCst) }\n",
        )
        .unwrap();
        let package_target = runtime.join("target");
        let output = Command::new(cargo_bin())
            .arg("package")
            .arg("--allow-dirty")
            .arg("--no-verify")
            .arg("--manifest-path")
            .arg(runtime.join("Cargo.toml"))
            .current_dir(runtime)
            .env_remove("CARGO")
            .env_remove("CARGO_MANIFEST_DIR")
            .env("CARGO_TARGET_DIR", &package_target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "failed to package registry runtime:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );

        let archive_name = format!("specgate-runtime-{REGISTRY_RUNTIME_VERSION}.crate");
        let archive = package_target.join("package").join(&archive_name);
        let archive = std::fs::read(&archive).unwrap();
        let checksum = format!("{:x}", Sha256::digest(&archive));
        let index_entry = serde_json::json!({
            "name": "specgate-runtime",
            "vers": REGISTRY_RUNTIME_VERSION,
            "deps": [],
            "cksum": checksum,
            "features": {},
            "yanked": false
        })
        .to_string();
        (index_entry, archive)
    }

    fn create_local_registry(registry: &Path, index_entry: &str, archive: &[u8]) {
        let index = registry.join("index");
        std::fs::create_dir_all(index.join("sp").join("ec")).unwrap();
        let archive_name = format!("specgate-runtime-{REGISTRY_RUNTIME_VERSION}.crate");
        std::fs::write(registry.join(archive_name), archive).unwrap();
        std::fs::write(index.join("sp").join("ec").join("specgate-runtime"), format!("{index_entry}\n")).unwrap();
    }

    fn create_registry_candidate(candidate: &Path, registry: &Path) {
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime=\"={REGISTRY_RUNTIME_VERSION}\"\n[workspace]\n"
            ),
        )
        .unwrap();
        std::fs::write(
            candidate.join("src").join("lib.rs"),
            "pub fn start_capture() { specgate_runtime::start_native_capture(); }\n",
        )
        .unwrap();
        let registry = toml::Value::String(cargo_path(registry).unwrap()).to_string();
        std::fs::write(
            candidate.join(".cargo").join("config.toml"),
            format!(
                "[source.crates-io]\nreplace-with=\"specgate-test-registry\"\n[source.specgate-test-registry]\nlocal-registry={registry}\n"
            ),
        )
        .unwrap();
    }

    fn create_alternate_registry_candidate(candidate: &Path, registry_index: &str) {
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={{version=\"={REGISTRY_RUNTIME_VERSION}\",registry=\"candidate-registry\"}}\n[workspace]\n"
            ),
        )
        .unwrap();
        std::fs::write(
            candidate.join("src").join("lib.rs"),
            "pub fn start_capture() { specgate_runtime::start_native_capture(); }\n",
        )
        .unwrap();
        std::fs::write(
            candidate.join(".cargo").join("config.toml"),
            format!(
                "[registries.candidate-registry]\nindex={}\n",
                toml::Value::String(registry_index.to_string())
            ),
        )
        .unwrap();
    }

    fn create_path_runtime(runtime: &Path, version: &str) {
        std::fs::create_dir_all(runtime.join("src")).unwrap();
        std::fs::write(
            runtime.join("Cargo.toml"),
            format!("[package]\nname=\"specgate-runtime\"\nversion=\"{version}\"\nedition=\"2024\"\n[workspace]\n"),
        )
        .unwrap();
        std::fs::write(
            runtime.join("src").join("lib.rs"),
            "use std::sync::atomic::{AtomicBool, Ordering};\nstatic CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);\npub fn start_native_capture() { CAPTURE_ACTIVE.store(true, Ordering::SeqCst); }\npub fn native_capture_is_active() -> bool { CAPTURE_ACTIVE.load(Ordering::SeqCst) }\n",
        )
        .unwrap();
    }

    fn create_path_candidate(candidate: &Path, runtime: &Path) {
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={{path={}}}\n[workspace]\n",
                toml::Value::String(cargo_path(runtime).unwrap())
            ),
        )
        .unwrap();
        std::fs::write(
            candidate.join("src").join("lib.rs"),
            "pub fn start_capture() { specgate_runtime::start_native_capture(); }\n",
        )
        .unwrap();
    }

    struct SparseRegistry {
        index: String,
        shutdown: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl SparseRegistry {
        fn start(index_entry: String, archive: Vec<u8>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let shutdown = Arc::new(AtomicBool::new(false));
            let server_shutdown = Arc::clone(&shutdown);
            let thread = std::thread::spawn(move || {
                while !server_shutdown.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _address)) => serve_registry_request(stream, address, &index_entry, &archive),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(error) => panic!("sparse registry server failed: {error}"),
                    }
                }
            });
            Self {
                index: format!("sparse+http://{address}/"),
                shutdown,
                thread: Some(thread),
            }
        }

        fn index(&self) -> &str {
            &self.index
        }
    }

    impl Drop for SparseRegistry {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let result = thread.join();
                if !std::thread::panicking() {
                    result.unwrap();
                }
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn sparse_registry_waits_for_request_bytes_after_accepting_a_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let mut client = TcpStream::connect(address).unwrap();
        let (stream, _) = listener.accept().unwrap();
        let server = std::thread::spawn(move || serve_registry_request(stream, address, "{}", &[]));

        std::thread::sleep(Duration::from_millis(50));
        client
            .write_all(b"GET /config.json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK"));
    }

    fn serve_registry_request(mut stream: TcpStream, address: SocketAddr, index_entry: &str, archive: &[u8]) {
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut request = [0_u8; 8192];
        let length = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..length]);
        let path = request.lines().next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("");
        let config = format!(r#"{{"dl":"http://{address}/api/v1/crates"}}"#);
        let (status, content_type, body) = match path {
            "/config.json" => ("200 OK", "application/json", config.as_bytes()),
            "/sp/ec/specgate-runtime" => ("200 OK", "text/plain", index_entry.as_bytes()),
            path if path == format!("/api/v1/crates/specgate-runtime/{REGISTRY_RUNTIME_VERSION}/download") => {
                ("200 OK", "application/octet-stream", archive)
            }
            _ => ("404 Not Found", "text/plain", &[] as &[u8]),
        };
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
    }
}
