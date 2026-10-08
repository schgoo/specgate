//! Typed Cargo package and generated-runner models.
//!
//! Package names, versions, locations, and resolved sources remain distinct so
//! generated manifests cannot silently exchange adjacent string values. A
//! [`CandidatePackage`] always carries the exact runtime source found in the
//! candidate graph; callers provide every required field through
//! [`CandidateDeps`]. [`Dependency::builder`] is for generated manifest entries
//! whose version and source are optional.
//!
//! ```
//! use std::path::PathBuf;
//! use specgate_discovery::runner::{CandidateDeps, CandidatePackage, Dependency, PackageSource};
//! let runtime = PackageSource::local("specgate-runtime", "0.6.0", "../runtime");
//! let candidate = CandidatePackage::new(CandidateDeps {
//!     package: "demo".into(),
//!     version: "1.0.0".into(),
//!     path: PathBuf::from("demo"),
//!     runtime,
//! });
//! assert_eq!(candidate.runtime.path(), Some(std::path::Path::new("../runtime")));
//! let dependency = Dependency::builder("serde").version("1").build()?;
//! assert_eq!(dependency.package.as_ref(), "serde");
//! # Ok::<(), specgate_discovery::Error>(())
//! ```

use crate::error::ErrorKind;
use crate::identity::{PackageName, PackageVersion, RegistryName};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn failure(message: impl Into<String>) -> crate::Error {
    crate::Error::message(ErrorKind::Cargo, message)
}

/// Exact Cargo source for the runtime package used by a candidate.
///
/// # Examples
///
/// ```
/// use specgate_discovery::runner::PackageSource;
/// let source = PackageSource::local("specgate-runtime", "0.6.0", "../runtime");
/// assert_eq!(source.path().unwrap().file_name().unwrap(), "runtime");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "Cargo identity DTOs intentionally expose every exact package field"
)]
pub struct PackageSource {
    /// Cargo package name.
    pub package: PackageName,
    /// Exact package version.
    pub version: PackageVersion,
    /// Mutually exclusive Cargo source location.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub location: PackageLocation,
}

/// Exact location of a resolved Cargo package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageLocation {
    /// A local path dependency.
    Path(PathBuf),
    /// A Cargo registry source.
    Registry(RegistryName),
}

#[cfg(feature = "serde")]
impl Serialize for PackageLocation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            path: Option<&'a Path>,
            #[serde(skip_serializing_if = "Option::is_none")]
            registry: Option<&'a str>,
        }
        match self {
            Self::Path(path) => Wire {
                path: Some(path),
                registry: None,
            },
            Self::Registry(registry) => Wire {
                path: None,
                registry: Some(registry.as_str()),
            },
        }
        .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for PackageLocation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            path: Option<PathBuf>,
            registry: Option<String>,
        }
        let wire = Wire::deserialize(deserializer)?;
        match (wire.path, wire.registry) {
            (Some(path), None) => Ok(Self::Path(path)),
            (None, Some(registry)) => Ok(Self::Registry(registry.into())),
            _ => Err(serde::de::Error::custom(
                "package source must contain exactly one of path or registry",
            )),
        }
    }
}

impl PackageSource {
    /// Construct an exact local package source.
    #[must_use]
    pub fn local(package: impl Into<PackageName>, version: impl Into<PackageVersion>, path: impl AsRef<Path>) -> Self {
        Self {
            package: package.into(),
            version: version.into(),
            location: PackageLocation::Path(path.as_ref().to_path_buf()),
        }
    }

    /// Construct an exact registry package source.
    #[must_use]
    pub fn registry(package: impl Into<PackageName>, version: impl Into<PackageVersion>, registry: impl Into<RegistryName>) -> Self {
        Self {
            package: package.into(),
            version: version.into(),
            location: PackageLocation::Registry(registry.into()),
        }
    }

    /// Return the local path, when path-sourced.
    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        match &self.location {
            PackageLocation::Path(path) => Some(path),
            PackageLocation::Registry(_) => None,
        }
    }

    /// Return the registry source, when registry-sourced.
    #[must_use]
    pub fn registry_source(&self) -> Option<&str> {
        match &self.location {
            PackageLocation::Path(_) => None,
            PackageLocation::Registry(registry) => Some(registry.as_str()),
        }
    }
}

/// Required values for constructing a resolved candidate package.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "Cargo identity DTOs intentionally expose every exact package field"
)]
pub struct CandidateDeps {
    /// Candidate package name.
    pub package: PackageName,
    /// Candidate package version.
    pub version: PackageVersion,
    /// Candidate package root.
    pub path: PathBuf,
    /// Exact runtime source resolved in the candidate graph.
    pub runtime: PackageSource,
}

/// Candidate package identity plus its exact resolved runtime source.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "Cargo identity DTOs intentionally expose every exact package field"
)]
pub struct CandidatePackage {
    /// Candidate package name.
    pub package: PackageName,
    /// Candidate package version.
    pub version: PackageVersion,
    /// Candidate package root.
    pub path: PathBuf,
    /// Exact runtime source resolved in the candidate graph.
    pub runtime: PackageSource,
}

impl CandidatePackage {
    /// Construct a resolved candidate package from all required values.
    #[must_use]
    pub fn new(deps: CandidateDeps) -> Self {
        Self {
            package: deps.package,
            version: deps.version,
            path: deps.path,
            runtime: deps.runtime,
        }
    }
}

/// Valid Cargo version requirement used in a generated dependency declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRequirement(Box<str>);
impl VersionRequirement {
    /// Parse a Cargo-compatible semantic-version requirement.
    ///
    /// # Errors
    /// Returns a Cargo error when the requirement is empty or malformed.
    pub fn try_new(value: impl Into<String>) -> Result<Self, crate::Error> {
        let value = value.into();
        semver::VersionReq::parse(&value).map_err(|error| failure(format!("invalid Cargo version requirement '{value}': {error}")))?;
        Ok(Self(value.into_boxed_str()))
    }
    /// Borrow the requirement's Cargo manifest spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Consume the requirement and return its Cargo manifest spelling.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0.into()
    }
}
impl std::ops::Deref for VersionRequirement {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}
impl AsRef<str> for VersionRequirement {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for VersionRequirement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One dependency in a generated Cargo manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "generated manifest DTOs intentionally expose every dependency field"
)]
pub struct Dependency {
    /// Cargo package name.
    pub package: PackageName,
    /// Optional exact or ranged Cargo version requirement.
    pub version: Option<VersionRequirement>,
    /// Optional explicit source.
    pub source: Option<DependencySource>,
}

/// Explicit source for a generated manifest dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencySource {
    /// A local package path.
    Path(PathBuf),
    /// A Cargo registry source.
    Registry(RegistryName),
}

/// Builder for a generated Cargo dependency.
#[must_use]
#[derive(Debug)]
pub struct DependencyBuilder {
    dependency: Dependency,
    version: Option<String>,
}

impl Dependency {
    /// Start a dependency builder with the required package identity.
    ///
    /// # Examples
    ///
    /// ```
    /// use specgate_discovery::runner::Dependency;
    /// let dependency = Dependency::builder("serde")
    ///     .version("1")
    ///     .build()?;
    /// assert_eq!(dependency.version.as_deref(), Some("1"));
    /// # Ok::<(), specgate_discovery::Error>(())
    /// ```
    pub fn builder(package: impl Into<PackageName>) -> DependencyBuilder {
        DependencyBuilder {
            dependency: Self {
                package: package.into(),
                version: None,
                source: None,
            },
            version: None,
        }
    }
    /// Build a generated dependency from an exact resolved package source.
    ///
    /// # Errors
    /// Returns an error when the resolved package version is not valid Cargo semver.
    pub fn from_source(source: &PackageSource) -> Result<Self, crate::Error> {
        Ok(Self {
            package: source.package.clone(),
            version: Some(VersionRequirement::try_new(format!("={}", source.version))?),
            source: Some(match &source.location {
                PackageLocation::Path(path) => DependencySource::Path(path.clone()),
                PackageLocation::Registry(registry) => DependencySource::Registry(registry.clone()),
            }),
        })
    }

    /// Build an exact local generated dependency.
    ///
    /// # Errors
    /// Returns an error when `version` is not valid Cargo semver.
    pub fn local(
        package: impl Into<PackageName>,
        version: impl Into<PackageVersion>,
        path: impl AsRef<Path>,
    ) -> Result<Self, crate::Error> {
        Ok(Self {
            package: package.into(),
            version: Some(VersionRequirement::try_new(format!("={}", version.into()))?),
            source: Some(DependencySource::Path(path.as_ref().to_path_buf())),
        })
    }
}

impl DependencyBuilder {
    /// Set an exact or ranged Cargo version requirement.
    pub fn version(mut self, value: impl Into<String>) -> Self {
        self.version = Some(value.into());
        self
    }
    /// Select a local package path.
    pub fn path(mut self, value: impl Into<PathBuf>) -> Self {
        self.dependency.source = Some(DependencySource::Path(value.into()));
        self
    }
    /// Select a named or URL-backed Cargo registry source.
    pub fn registry(mut self, value: impl Into<String>) -> Self {
        self.dependency.source = Some(DependencySource::Registry(value.into().into()));
        self
    }
    /// Build and validate the dependency.
    ///
    /// # Errors
    ///
    /// Rejects empty package, version, path, or registry values, and malformed nonempty version requirements.
    pub fn build(mut self) -> Result<Dependency, crate::Error> {
        if self.dependency.package.trim().is_empty() {
            return Err(failure("generated Cargo dependency package must not be empty"));
        }
        if let Some(version) = self.version {
            self.dependency.version = Some(VersionRequirement::try_new(version)?);
        }
        match &self.dependency.source {
            Some(DependencySource::Path(path)) if path.as_os_str().is_empty() => {
                return Err(failure("generated Cargo dependency path must not be empty"));
            }
            Some(DependencySource::Registry(registry)) if registry.trim().is_empty() => {
                return Err(failure("generated Cargo dependency registry must not be empty"));
            }
            _ => {}
        }
        Ok(self.dependency)
    }
}

/// Generated Cargo files for an isolated runner.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "generated runner files are an exhaustively consumed internal protocol"
)]
pub struct RunnerCargo {
    /// Generated `Cargo.toml` contents.
    pub manifest: String,
    /// Optional generated Cargo configuration.
    pub config: Option<String>,
}
