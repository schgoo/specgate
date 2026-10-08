//! Serializable candidate replay plan data.
//!
//! A [`Plan`] binds exact registry bytes to one resolved candidate target, a
//! deduplicated operation-link table, and ordered scenario invocations. Strong
//! identity types prevent package, component, operation, registry, and digest
//! values from being exchanged accidentally while transparent serialization
//! preserves the established replay-plan JSON.
use super::*;
use specgate_discovery::identity::{ComponentId, OperationName, PackageName, PackageVersion, TargetName};

/// Candidate implementation language encoded in replay plans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Language {
    Rust,
    #[serde(rename = "csharp")]
    CSharp,
}
impl Language {
    /// Return the stable lowercase wire spelling.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::CSharp => "csharp",
        }
    }
}
impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl From<specgate_discovery::binding::Language> for Language {
    fn from(value: specgate_discovery::binding::Language) -> Self {
        match value {
            specgate_discovery::binding::Language::Rust => Self::Rust,
            specgate_discovery::binding::Language::CSharp => Self::CSharp,
        }
    }
}

/// A CTSC registry family identity preserved exactly on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct RegistryId(String);
impl RegistryId {
    /// Borrow the exact preserved wire value.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RegistryId {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl std::fmt::Display for RegistryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A CTSC registry source version preserved exactly on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct RegistryVersion(String);
impl RegistryVersion {
    /// Borrow the exact preserved wire value.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RegistryVersion {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl std::fmt::Display for RegistryVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// Number of lowercase hexadecimal characters in a SHA-256 digest.
const SHA256_WIDTH: usize = 64;

/// The verified SHA-256 digest identifying exact registry bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct RegistryDigest(String);
impl RegistryDigest {
    /// Borrow the verified lowercase digest spelling.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    /// Validate and construct a `sha256:` digest.
    ///
    /// Returns a replay error unless exactly 64 lowercase hexadecimal digits follow the prefix.
    pub(crate) fn try_new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        let valid = value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == SHA256_WIDTH && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        valid
            .then_some(Self(value))
            .ok_or_else(|| Error::from("registry digest must be 'sha256:' followed by 64 lowercase hexadecimal characters"))
    }
}
impl TryFrom<String> for RegistryDigest {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}
impl<'de> Deserialize<'de> for RegistryDigest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_from(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl std::fmt::Display for RegistryDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One candidate target described by a serializable replay plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Target {
    /// Binding target name.
    pub(crate) name: TargetName,
    /// Candidate implementation language.
    pub(crate) language: Language,
    /// Cargo package name.
    pub(crate) package_name: PackageName,
    /// Cargo package version.
    pub(crate) package_version: PackageVersion,
    /// Absolute package root used to generate the runner.
    pub(crate) package_root: PathBuf,
    /// Exact resolved runtime package source.
    pub(crate) runtime: specgate_discovery::runner::PackageSource,
}

/// One ordered candidate parameter in a statically linked invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Input {
    /// Semantic input name.
    pub(crate) name: String,
    /// CTSC semantic input type.
    pub(crate) semantic_type: ReplayType,
    /// Candidate Rust parameter type.
    pub(crate) rust_type: String,
}

/// One distinct semantic-to-candidate operation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Link {
    /// Owning component identifier.
    pub(crate) component_id: ComponentId,
    /// Semantic operation name.
    pub(crate) operation_name: OperationName,
    /// Candidate Rust module path.
    pub(crate) module_path: Vec<String>,
    /// Candidate Rust function name.
    pub(crate) fn_name: String,
    /// Ordered candidate inputs.
    pub(crate) inputs: Vec<Input>,
    /// Optional candidate output type.
    pub(crate) output: Option<ReplayType>,
}

/// One planned invocation with scenario-specific semantic values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlannedOp {
    /// Index into the plan's distinct operation links.
    pub(crate) link_index: usize,
    /// Scenario-specific semantic inputs.
    pub(crate) inputs: Vec<ReplayInput>,
}

/// One ordered candidate replay scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Scenario {
    /// Stable scenario name.
    pub(crate) name: String,
    /// Zero-based scenario order.
    pub(crate) index: u64,
    /// Ordered top-level operations to invoke.
    pub(crate) operations: Vec<PlannedOp>,
}

/// The complete target-local invocation plan built before code generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Plan {
    /// Replayed component identifier.
    pub(crate) component_id: ComponentId,
    /// Linked registry identifier.
    pub(crate) registry_id: RegistryId,
    /// Linked registry version.
    pub(crate) registry_version: RegistryVersion,
    /// Linked registry digest.
    pub(crate) registry_digest: RegistryDigest,
    /// Resolved candidate target.
    pub(crate) target: Target,
    /// Distinct semantic-to-candidate operation links.
    pub(crate) links: Vec<Link>,
    /// Ordered replay scenarios.
    pub(crate) scenarios: Vec<Scenario>,
}

/// One component schema paired with shared resolved candidate metadata.
#[derive(Debug, Clone)]
pub(super) struct Candidate {
    /// Shared package, runtime, and raw registry metadata for the invocation.
    pub(super) inner: Arc<CandidatesInner>,
    /// Normalized schema for the selected component.
    pub(super) schema: Schema,
}
impl Candidate {
    /// Borrow shared candidate metadata explicitly.
    pub(super) fn metadata(&self) -> &CandidatesInner {
        &self.inner
    }
}

#[cfg(test)]
mod tests {
    use super::{Language, Plan, RegistryDigest, RegistryId, RegistryVersion, SHA256_WIDTH, Target};
    use specgate_discovery::identity::{ComponentId, PackageName, PackageVersion, TargetName};
    use specgate_discovery::runner::PackageSource;
    use std::path::PathBuf;

    #[test]
    fn wire_roundtrip() {
        RegistryDigest::try_from("sha256:abcd".to_string()).unwrap_err();
        RegistryDigest::try_from(format!("sha256:{}", "A".repeat(SHA256_WIDTH))).unwrap_err();
        let values = [
            serde_json::to_value(RegistryId::from("registry".to_string())).unwrap(),
            serde_json::to_value(RegistryVersion::from("1.0.0".to_string())).unwrap(),
            serde_json::to_value(RegistryDigest::try_from(format!("sha256:{}", "a".repeat(64))).unwrap()).unwrap(),
            serde_json::to_value(Language::Rust).unwrap(),
        ];
        let expected = [
            serde_json::Value::from("registry"),
            serde_json::Value::from("1.0.0"),
            serde_json::Value::from(format!("sha256:{}", "a".repeat(64))),
            serde_json::Value::from("rust"),
        ];
        assert_eq!(values, expected);
        assert_eq!(
            serde_json::from_value::<RegistryId>(values[0].clone()).unwrap().as_str(),
            "registry"
        );
        assert_eq!(
            serde_json::from_value::<RegistryVersion>(values[1].clone()).unwrap().as_str(),
            "1.0.0"
        );
        assert_eq!(
            serde_json::from_value::<RegistryDigest>(values[2].clone()).unwrap().as_str(),
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(serde_json::from_value::<Language>(values[3].clone()).unwrap(), Language::Rust);
    }

    #[test]
    fn plan_roundtrip() {
        let plan = Plan {
            component_id: ComponentId::from("fixture.component"),
            registry_id: RegistryId::from("urn:ctsc:registry:fixture.component".to_string()),
            registry_version: RegistryVersion::from("0.1.0".to_string()),
            registry_digest: RegistryDigest::try_new(format!("sha256:{}", "a".repeat(64))).unwrap(),
            target: Target {
                name: TargetName::from("fixture"),
                language: Language::Rust,
                package_name: PackageName::from("fixture"),
                package_version: PackageVersion::from("1.0.0"),
                package_root: PathBuf::from("fixture"),
                runtime: PackageSource::local("specgate-runtime", "0.6.0", "../runtime"),
            },
            links: Vec::new(),
            scenarios: Vec::new(),
        };
        let json = serde_json::to_value(&plan).unwrap();
        assert_eq!(serde_json::from_value::<Plan>(json.clone()).unwrap(), plan);
        assert_eq!(json["registry_digest"], format!("sha256:{}", "a".repeat(64)));
    }
}
