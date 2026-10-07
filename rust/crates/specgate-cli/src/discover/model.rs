//! Discovery request and report models with stable CTSC wire identities.
//!
//! # Examples
//! ```
//! use specgate_cli::discover::{ComponentName, Params, Request, RegistryId, RegistryVersion};
//! let request = Request::builder(Params {
//!     binding: "binding.yaml".into(), out: "registry.json".into(),
//!     component: ComponentName::parse("example.math")?,
//!     registry_id: RegistryId::parse("registry-id")?,
//!     registry_version: RegistryVersion::parse("1")?,
//! }).target("rust").build()?;
//! assert_eq!(request.component(), "example.math");
//! # Ok::<(), specgate_cli::discover::Error>(())
//! ```
use super::{Error, Path, PathBuf};
use specgate::{ComponentId, SpecEvent, TargetName};

/// Component identity supplied to discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentName(String);
impl ComponentName {
    /// Parse a non-empty component identity.
    ///
    /// # Errors
    /// Returns a request error when `value` is empty.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(Error::request("discover requires a non-empty component identifier"));
        }
        Ok(Self(value.to_owned()))
    }
    /// Return the parsed component identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for ComponentName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Registry document identity supplied to CTSC encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryId(String);
impl RegistryId {
    /// Parse a non-empty registry identity.
    ///
    /// # Errors
    /// Returns a request error when `value` is empty.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(Error::request("discover requires a non-empty registry identifier"));
        }
        Ok(Self(value.to_owned()))
    }
    /// Borrow the validated registry value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RegistryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Registry schema version supplied to CTSC encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryVersion(String);
impl RegistryVersion {
    /// Parse a non-empty registry version.
    ///
    /// # Errors
    /// Returns a request error when `value` is empty.
    pub fn parse(value: impl AsRef<str>) -> Result<Self, Error> {
        let value = value.as_ref();
        if value.is_empty() {
            return Err(Error::request("discover requires a non-empty registry version"));
        }
        Ok(Self(value.to_owned()))
    }
    /// Borrow the validated registry value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RegistryVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Required values for one registry discovery.
///
/// Paths remain operating-system paths; component and registry identity are
/// supplied up front because discovery cannot succeed without them.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery parameters are an intentionally direct public construction DTO"
)]
pub struct Params {
    /// Binding document path.
    pub binding: PathBuf,
    /// Destination registry path.
    pub out: PathBuf,
    /// Component selected from the binding target.
    pub component: ComponentName,
    /// Published registry identity.
    pub registry_id: RegistryId,
    /// Published registry schema version.
    pub registry_version: RegistryVersion,
}

/// Owned inputs for one discovery operation.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "DiscoverRequest")]
pub struct Request {
    #[spec_event(path)]
    binding: PathBuf,
    #[spec_event]
    target: TargetName,
    #[spec_event]
    component: ComponentId,
    registry_id: RegistryId,
    #[spec_event(name = "registry_id")]
    id_event: String,
    registry_version: RegistryVersion,
    #[spec_event(name = "registry_version")]
    version_event: String,
    #[spec_event(path)]
    out: PathBuf,
}

/// Staged construction for [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct RequestBuilder {
    binding: PathBuf,
    target: TargetName,
    component: ComponentId,
    registry_id: RegistryId,
    registry_version: RegistryVersion,
    out: PathBuf,
}

impl Request {
    /// Begin a request with all required discovery values.
    pub fn builder(params: impl Into<Params>) -> RequestBuilder {
        let params = params.into();
        RequestBuilder {
            binding: params.binding,
            target: TargetName::default(),
            component: ComponentId::from(params.component.as_str()),
            registry_id: params.registry_id,
            registry_version: params.registry_version,
            out: params.out,
        }
    }
    /// Binding path.
    #[must_use]
    pub fn binding(&self) -> &Path {
        &self.binding
    }
    /// Target name, empty for the binding default.
    #[must_use]
    pub fn target(&self) -> &str {
        self.target.as_str()
    }
    /// Component identifier.
    #[must_use]
    pub fn component(&self) -> &str {
        self.component.as_str()
    }
    pub(super) fn component_name(&self) -> ComponentName {
        ComponentName(self.component.to_string())
    }
    /// Registry identifier.
    #[must_use]
    pub fn registry_id(&self) -> &str {
        self.registry_id.as_str()
    }
    /// Registry version.
    #[must_use]
    pub fn registry_version(&self) -> &str {
        self.registry_version.as_str()
    }
    /// Output path.
    #[must_use]
    pub fn out(&self) -> &Path {
        &self.out
    }
}

impl RequestBuilder {
    /// Select a binding target.
    pub fn target(mut self, value: impl AsRef<str>) -> Self {
        self.target = TargetName::from(value.as_ref());
        self
    }
    /// Validate and construct the request.
    ///
    /// # Errors
    /// Returns [`Error`] when a required value is empty or a path is not Unicode.
    pub fn build(self) -> Result<Request, Error> {
        if self.binding.as_os_str().is_empty() {
            return Err(Error::request("discover requires a non-empty binding path"));
        }
        if self.out.as_os_str().is_empty() {
            return Err(Error::request("discover requires a non-empty output path"));
        }
        Ok(Request {
            binding: self.binding,
            target: self.target,
            component: self.component,
            id_event: self.registry_id.to_string(),
            version_event: self.registry_version.to_string(),
            registry_id: self.registry_id,
            registry_version: self.registry_version,
            out: self.out,
        })
    }
}

/// Summary of a discovery run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_event(name = "DiscoverReport")]
pub struct Report {
    /// Discovered component identifier.
    pub component_id: ComponentName,
    #[spec_event(name = "component_id")]
    pub(super) component_event: String,
    /// Number of operations in the registry.
    #[spec_event]
    pub operations: i32,
    /// Number of named types in the registry.
    #[spec_event]
    pub types: i32,
    /// Written registry path.
    #[spec_event(path)]
    pub output_path: PathBuf,
}

/// Stable source spelling consumed by operation metadata.
pub(super) type DiscoverRequest = Request;
/// Stable source spelling consumed by operation metadata.
pub(super) type DiscoverReport = Report;
