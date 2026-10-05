//! Cargo package identity and generated-runner metadata.
//!
//! # Example
//! ```no_run
//! use specgate_discovery::runner::Dependency;
//! let dependency = Dependency::builder("demo").version("1.0.0").build()?;
//! assert_eq!(dependency.package, "demo");
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

//! Cache lifecycle, Cargo metadata identity, and generated manifests.

//! Runtime cache, Cargo source resolution, and generated manifest support.

use crate::error::{Error, ErrorKind};
mod models;
pub use models::{
    CandidateDeps, CandidatePackage, Dependency, DependencyBuilder, DependencySource, PackageLocation, PackageSource, RunnerCargo,
    VersionRequirement,
};

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Cargo, message)
}

fn context(message: impl Into<String>, source: Error) -> Error {
    Error::cause(ErrorKind::Cargo, message, source)
}

const FACADE_PACKAGE: &str = "specgate";
const RUNTIME_PACKAGE: &str = "specgate-runtime";

#[cfg(test)]
use super::cache::{CacheLabel, CacheScope, InvocationCache, root_for};
use super::system;

// Generated helper packages are private and unpublished; these values only
// select the manifest schema and language edition used by their source.
const PACKAGE_VERSION: &str = "0.0.0";
const PACKAGE_EDITION: &str = "2024";
// Cargo metadata format 1 is the stable shape deserialized below.
const METADATA_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuntimeOverride {
    Path { package_id: String },
    Version(String),
}

/// Resolve the candidate and exact `specgate-runtime` source through
/// `cargo metadata` rooted at the candidate package.
///
/// Explicit overrides are available through `SPECGATE_RUNTIME_PATH` or
/// `SPECGATE_RUNTIME_VERSION`; setting both is an error. Overrides must select
/// the same Cargo `PackageId` already resolved by the candidate. No caller-CWD
/// source discovery is performed.
///
/// With the `test-util` feature, [`crate::test_util::candidate_with`] provides
/// the supported public injection seam for filesystem, process, and environment behavior.
///
/// # Errors
///
/// Returns metadata, graph, ambiguity, unsupported-source, or override errors.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::runner::candidate_package;
///
/// let package = candidate_package("fixtures/rust-library")?;
/// assert_eq!(package.package.as_str(), "rust-library");
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn candidate_package(package_root: impl AsRef<Path>) -> Result<CandidatePackage, Error> {
    candidate_in(package_root.as_ref(), &system::System::real())
}

pub(crate) fn candidate_in(package_root: &Path, system: &system::System) -> Result<CandidatePackage, Error> {
    let runtime_override = runtime_override(system)?;
    override_context(package_root, runtime_override.as_ref(), system)
}

#[cfg(test)]
fn context_override(package_root: &Path, runtime_override: Option<&RuntimeOverride>) -> Result<CandidatePackage, Error> {
    override_context(package_root, runtime_override, &system::System::real())
}

fn override_context(
    package_root: &Path,
    runtime_override: Option<&RuntimeOverride>,
    system: &system::System,
) -> Result<CandidatePackage, Error> {
    let manifest = system
        .filesystem
        .canonicalize(package_root.join("Cargo.toml"))
        .map_err(|source| context(format!("cannot resolve candidate Cargo.toml: {source}"), source))?;
    let package_root = manifest.parent().ok_or_else(|| failure("candidate Cargo.toml has no parent"))?;
    let metadata = cargo_metadata(&manifest, package_root, system)?;
    let candidate = metadata
        .packages
        .iter()
        .find(|package| same_path(&package.manifest_path, &manifest, system))
        .ok_or_else(|| failure(format!("cargo metadata did not return candidate package {}", manifest.display())))?;
    let runtime_package = resolve_runtime(&metadata, &candidate.id)?;
    let runtime = match runtime_override {
        Some(runtime_override) => override_source(runtime_package, runtime_override)?,
        None => package_source(runtime_package)?,
    };
    Ok(CandidatePackage::new(CandidateDeps {
        package: candidate.name.clone().into(),
        version: candidate.version.clone().into(),
        path: package_root.to_path_buf(),
        runtime,
    }))
}

/// Serialize a standalone generated Cargo manifest and any registry config
/// needed to preserve dependency source identity.
///
/// # Errors
///
/// Returns an error for invalid sources, non-UTF-8 paths, alias collisions, or
/// TOML serialization failure.
///
/// # Examples
///
/// ```
/// use specgate_discovery::runner::{Dependency, runner_cargo};
/// use std::collections::BTreeMap;
///
/// let dependencies = BTreeMap::from([(
///     "demo".to_string(),
///     Dependency::builder("demo").version("1.0.0").build()?,
/// )]);
/// let cargo = runner_cargo("generated-runner", dependencies)?;
/// assert!(cargo.manifest.contains("generated-runner"));
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn runner_cargo(package_name: impl AsRef<str>, dependencies: BTreeMap<String, Dependency>) -> Result<RunnerCargo, Error> {
    let package_name = package_name.as_ref();
    let mut registries = BTreeMap::new();
    let dependencies = dependencies
        .into_iter()
        .map(|(alias, dependency)| {
            let path = match &dependency.source {
                Some(DependencySource::Path(path)) => Some(cargo_path(path)?),
                _ => None,
            };
            let registry = match &dependency.source {
                Some(DependencySource::Registry(registry)) => Some(registry.as_str()),
                _ => None,
            }
            .filter(|source| !crates_io(source))
            .map(|source| {
                let registry_alias = registry_alias(source);
                let index = registry_index(source)?.to_string();
                if let Some(existing) = registries.insert(registry_alias.clone(), SerializableRegistry { index: index.clone() })
                    && existing.index != index
                {
                    return Err(failure(format!("generated Cargo registry alias collision for source '{source}'")));
                }
                Ok(registry_alias)
            })
            .transpose()?;
            Ok((
                alias,
                SerializableDependency {
                    package: dependency.package.into_string(),
                    version: dependency.version.map(VersionRequirement::into_string),
                    path,
                    registry,
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let mut manifest = toml::to_string(&GeneratedManifest {
        package: GeneratedPackage {
            name: package_name,
            version: PACKAGE_VERSION,
            edition: PACKAGE_EDITION,
            publish: false,
        },
        dependencies,
    })
    .map_err(|source| {
        Error::cause(
            ErrorKind::Cargo,
            format!("failed to serialize generated Cargo manifest: {source}"),
            source,
        )
    })?;
    manifest.push_str("\n[workspace]\n");
    manifest.shrink_to_fit();
    let config = if registries.is_empty() {
        None
    } else {
        let mut config = toml::to_string(&CargoConfig { registries }).map_err(|source| {
            Error::cause(
                ErrorKind::Cargo,
                format!("failed to serialize generated Cargo registry config: {source}"),
                source,
            )
        })?;
        config.shrink_to_fit();
        Some(config)
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
struct CargoConfig {
    registries: BTreeMap<String, SerializableRegistry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SerializableRegistry {
    index: String,
}

#[derive(Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    resolve: Option<CargoResolve>,
}

#[derive(Deserialize)]
struct CargoPackage {
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
    deps: Vec<NodeDependency>,
}

#[derive(Deserialize)]
struct NodeDependency {
    pkg: String,
}

fn cargo_metadata(manifest: &Path, package_root: &Path, system: &system::System) -> Result<CargoMetadata, Error> {
    let request = system::ProcessRequest::builder(cargo_with(system))
        .arg("metadata")
        .arg("--format-version")
        .arg(METADATA_VERSION)
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(package_root)
        .build();
    let output = system
        .process
        .output(&request)
        .map_err(|source| context(format!("failed to invoke cargo metadata: {source}"), source))?;
    if !output.success {
        return Err(failure(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|source| Error::cause(ErrorKind::Cargo, format!("cargo metadata output was malformed: {source}"), source))
}

fn resolve_runtime<'a>(metadata: &'a CargoMetadata, candidate_id: &str) -> Result<&'a CargoPackage, Error> {
    let resolve = metadata
        .resolve
        .as_ref()
        .ok_or_else(|| failure("cargo metadata returned no resolved dependency graph"))?;
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
        .ok_or_else(|| failure("candidate package is absent from cargo metadata resolve graph"))?;

    let direct_specgate = candidate
        .deps
        .iter()
        .filter_map(|dependency| packages.get(dependency.pkg.as_str()).copied())
        .find(|package| package.name == FACADE_PACKAGE);
    let start = direct_specgate.map_or(candidate_id, |package| package.id.as_str());

    let mut queue = VecDeque::from([start]);
    let mut visited = BTreeSet::new();
    let mut runtimes = BTreeSet::new();
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(package) = packages.get(id)
            && package.name == RUNTIME_PACKAGE
        {
            runtimes.insert(id);
            continue;
        }
        if let Some(node) = nodes.get(id) {
            queue.extend(node.deps.iter().map(|dependency| dependency.pkg.as_str()));
        }
    }
    let runtime_id = match runtimes.into_iter().collect::<Vec<_>>().as_slice() {
        [] => return Err(failure("candidate dependency graph contains no specgate-runtime package")),
        [runtime] => *runtime,
        runtimes => {
            return Err(failure(format!(
                "candidate dependency graph contains ambiguous specgate-runtime packages: {}",
                runtimes.join(", ")
            )));
        }
    };
    Ok(*packages
        .get(runtime_id)
        .expect("runtime IDs are collected only for packages present in metadata"))
}

fn runtime_override(system: &system::System) -> Result<Option<RuntimeOverride>, Error> {
    let path = env_path("SPECGATE_RUNTIME_PATH", system);
    let version = system
        .environment
        .var("SPECGATE_RUNTIME_VERSION")
        .ok()
        .filter(|value| !value.is_empty());
    match (path, version) {
        (Some(_), Some(_)) => Err(failure("set only one of SPECGATE_RUNTIME_PATH or SPECGATE_RUNTIME_VERSION")),
        (Some(path), None) => path_override(&path, system).map(Some),
        (None, Some(version)) => Ok(Some(RuntimeOverride::Version(version))),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
fn local_override(path: &Path) -> Result<RuntimeOverride, Error> {
    path_override(path, &system::System::real())
}

fn path_override(path: &Path, system: &system::System) -> Result<RuntimeOverride, Error> {
    let manifest = system
        .filesystem
        .canonicalize(path.join("Cargo.toml"))
        .map_err(|source| context(format!("invalid SPECGATE_RUNTIME_PATH: {source}"), source))?;
    let package_root = manifest
        .parent()
        .ok_or_else(|| failure("SPECGATE_RUNTIME_PATH Cargo.toml has no parent"))?;
    let metadata = cargo_metadata(&manifest, package_root, system)?;
    let package = metadata
        .packages
        .iter()
        .find(|package| same_path(&package.manifest_path, &manifest, system) && package.name == RUNTIME_PACKAGE)
        .ok_or_else(|| failure("SPECGATE_RUNTIME_PATH is not a specgate-runtime package"))?;
    Ok(RuntimeOverride::Path {
        package_id: package.id.clone(),
    })
}

fn override_source(candidate_runtime: &CargoPackage, runtime_override: &RuntimeOverride) -> Result<PackageSource, Error> {
    match runtime_override {
        RuntimeOverride::Path { package_id } if package_id == &candidate_runtime.id => package_source(candidate_runtime),
        RuntimeOverride::Path { package_id } => Err(failure(format!(
            "SPECGATE_RUNTIME_PATH resolves to Cargo PackageId '{package_id}', which is a different Cargo PackageId from the \
             candidate's specgate-runtime '{}'; using both would create distinct runtime/linkme/capture state; remove the override or \
             align the candidate dependency",
            candidate_runtime.id
        ))),
        RuntimeOverride::Version(version) if version != &candidate_runtime.version => Err(failure(format!(
            "SPECGATE_RUNTIME_VERSION={version} does not match: the candidate resolves specgate-runtime version {} as Cargo PackageId \
             '{}'; remove the override or align the candidate dependency",
            candidate_runtime.version, candidate_runtime.id
        ))),
        RuntimeOverride::Version(version) => match candidate_runtime.source.as_deref() {
            Some(source) if crates_io(source) => package_source(candidate_runtime),
            _ => Err(failure(format!(
                "SPECGATE_RUNTIME_VERSION={version} selects crates.io, but the candidate resolves specgate-runtime as Cargo PackageId \
                 '{}'; using both would create distinct runtime/linkme/capture state; remove the override or use \
                 SPECGATE_RUNTIME_PATH only when it resolves to that exact PackageId",
                candidate_runtime.id
            ))),
        },
    }
}

fn package_source(package: &CargoPackage) -> Result<PackageSource, Error> {
    let resolved_path = package
        .manifest_path
        .parent()
        .ok_or_else(|| {
            failure(format!(
                "resolved package Cargo.toml has no parent: {}",
                package.manifest_path.display()
            ))
        })?
        .to_path_buf();
    let location = match package.source.as_deref() {
        None => PackageLocation::Path(resolved_path),
        Some(source) if registry_index(source).is_ok() => PackageLocation::Registry(source.into()),
        Some(source) => {
            return Err(failure(format!(
                "unsupported specgate-runtime Cargo source '{source}' for PackageId '{}'; generated runners cannot preserve this \
                 identity without a graph-wide Cargo source replacement",
                package.id
            )));
        }
    };
    Ok(PackageSource {
        package: package.name.clone().into(),
        version: package.version.clone().into(),
        location,
    })
}

fn registry_index(source: &str) -> Result<&str, Error> {
    if let Some(index) = source.strip_prefix("registry+") {
        if index.is_empty() {
            return Err(failure("registry source URL is empty"));
        }
        return Ok(index);
    }
    if source.starts_with("sparse+") && source.len() > "sparse+".len() {
        return Ok(source);
    }
    Err(failure(format!("unsupported Cargo registry source '{source}'")))
}

fn crates_io(source: &str) -> bool {
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
    // FNV-1a keeps aliases deterministic across processes and platforms.
    // Changing these constants invalidates generated registry aliases.
    const FNV1A_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV1A_PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = source
        .as_bytes()
        .iter()
        .fold(FNV1A_OFFSET_BASIS, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(FNV1A_PRIME));
    format!("specgate-source-{hash:016x}")
}

fn env_path(name: &str, system: &system::System) -> Option<PathBuf> {
    system.environment.var_os(name).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn same_path(left: &Path, right: &Path, system: &system::System) -> bool {
    system.filesystem.canonicalize(left).unwrap_or_else(|_| left.to_path_buf())
        == system.filesystem.canonicalize(right).unwrap_or_else(|_| right.to_path_buf())
}

fn cargo_path(path: &Path) -> Result<String, Error> {
    let value = path
        .to_str()
        .ok_or_else(|| failure(format!("Cargo dependency path is not UTF-8: {}", path.display())))?;
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

#[cfg(test)]
fn cargo_bin() -> OsString {
    cargo_with(&system::System::real())
}
pub(crate) fn cargo_with(system: &system::System) -> OsString {
    system
        .environment
        .var_os(OsStr::new("CARGO"))
        .unwrap_or_else(|| OsString::from("cargo"))
}

#[cfg(test)]
mod tests;
