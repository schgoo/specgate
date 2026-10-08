//! Private capture manifest and execution data transfer objects.
use super::{ArtifactName, ComponentId, FailureContext, PathBuf, Serialize};
/// Hexadecimal width of a 32-byte SHA-256 digest.
const SHA256_WIDTH: usize = 64;
use specgate_discovery::identity::TargetName;

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(super) struct Language(&'static str);
impl From<specgate_discovery::binding::Language> for Language {
    fn from(value: specgate_discovery::binding::Language) -> Self {
        Self(value.as_str())
    }
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(super) struct RegistryId(String);
impl RegistryId {
    fn try_new(value: String) -> Result<Self, FailureContext> {
        (!value.trim().is_empty())
            .then_some(Self(value))
            .ok_or_else(|| FailureContext::domain("manifest registry identity must not be empty"))
    }
}
impl TryFrom<String> for RegistryId {
    type Error = FailureContext;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(super) struct WireDigest(String);
impl WireDigest {
    fn try_new(value: String) -> Result<Self, FailureContext> {
        let valid = value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == SHA256_WIDTH && hex.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        valid
            .then_some(Self(value))
            .ok_or_else(|| FailureContext::domain("manifest digest must be a lowercase SHA-256 identity"))
    }
}
impl TryFrom<String> for WireDigest {
    type Error = FailureContext;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Debug, Serialize)]
#[serde(transparent)]
pub(super) struct Version(&'static str);
impl Version {
    fn try_new(value: &'static str) -> Result<Self, FailureContext> {
        (!value.trim().is_empty())
            .then_some(Self(value))
            .ok_or_else(|| FailureContext::domain("manifest version must not be empty"))
    }
}
impl TryFrom<&'static str> for Version {
    type Error = FailureContext;
    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Debug)]
pub(super) struct TestBinary {
    pub(super) label: String,
    pub(super) executable: PathBuf,
    pub(super) is_library: bool,
}

#[derive(Debug)]
pub(super) struct IsolatedTest {
    pub(super) scenario_name: String,
    pub(super) test_name: String,
    pub(super) executable: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct CaptureManifest {
    pub(super) format: &'static str,
    pub(super) format_version: Version,
    pub(super) component_id: ComponentId,
    pub(super) target: TargetDto,
    pub(super) tool: Tool,
    pub(super) registry: RegistryDto,
    pub(super) reference: Reference,
    pub(super) scenarios: Scenarios,
}

#[derive(Debug, Serialize)]
pub(super) struct TargetDto {
    pub(super) name: TargetName,
    pub(super) language: Language,
}

#[derive(Debug, Serialize)]
pub(super) struct Tool {
    pub(super) name: &'static str,
    pub(super) version: Version,
}

#[derive(Debug, Serialize)]
pub(super) struct RegistryDto {
    pub(super) path: ArtifactName,
    pub(super) id: RegistryId,
    pub(super) version: Version,
    pub(super) digest: WireDigest,
}

#[derive(Debug, Serialize)]
pub(super) struct Reference {
    pub(super) path: ArtifactName,
    pub(super) digest: WireDigest,
}

#[derive(Debug, Serialize)]
pub(super) struct Scenarios {
    pub(super) count: usize,
    pub(super) names: Vec<String>,
}
