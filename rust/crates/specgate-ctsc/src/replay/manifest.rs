//! Capture-manifest wire model and contract validation.

use super::{BTreeSet, Deserialize, MANIFEST_FORMAT, MANIFEST_VERSION, error};

// Fixed capture-bundle paths shared by producers, validators, and replay consumers.
const REGISTRY_PATH: &str = "registry.ctsc.json";
const REFERENCE_PATH: &str = "reference.otlp.json";

/// Capture bundle manifest wire document; validated by [`validate_manifest`].
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Manifest {
    /// Stable manifest format identifier.
    pub(super) format: String,
    /// Supported manifest format version.
    pub(super) format_version: String,
    /// Non-empty component identity from captured registry metadata.
    pub(super) component_id: String,
    /// Captured binding target.
    pub(super) target: ManifestTarget,
    /// Capture producer.
    pub(super) tool: ManifestTool,
    /// Registry artifact declaration.
    pub(super) registry: ManifestRegistry,
    /// Reference trace declaration.
    pub(super) reference: ManifestReference,
    /// Captured scenario declaration.
    pub(super) scenarios: ManifestScenarios,
}

/// Captured implementation target identity.
#[derive(Deserialize)]
pub(super) struct ManifestTarget {
    /// Non-empty binding target name.
    pub(super) name: String,
    /// Non-empty binding language identifier.
    pub(super) language: String,
}

/// Producer identity and semantic version.
#[derive(Deserialize)]
pub(super) struct ManifestTool {
    /// Non-empty producer name.
    pub(super) name: String,
    /// Non-empty producer version.
    pub(super) version: String,
}

/// Registry artifact linkage and identity.
#[derive(Deserialize)]
pub(super) struct ManifestRegistry {
    /// Fixed bundle-relative registry path.
    pub(super) path: String,
    /// Non-empty registry identity.
    pub(super) id: String,
    /// Non-empty registry version.
    pub(super) version: String,
    /// Registry content digest.
    pub(super) digest: Digest,
}

/// Reference trace artifact linkage.
#[derive(Deserialize)]
pub(super) struct ManifestReference {
    /// Fixed bundle-relative reference trace path.
    pub(super) path: String,
    /// Reference trace content digest.
    pub(super) digest: Digest,
}

/// Declared scenario inventory.
#[derive(Deserialize)]
pub(super) struct ManifestScenarios {
    /// Positive scenario count, checked against `names`.
    pub(super) count: i32,
    /// Unique scenario names in capture order.
    pub(super) names: Vec<String>,
}

/// Manifest data safe for replay after all wire invariants are checked.
pub(super) struct ValidatedManifest {
    pub(super) component_id: String,
    pub(super) registry: ValidatedRegistry,
    pub(super) reference_digest: Digest,
    pub(super) scenario_names: Vec<String>,
}

/// Registry linkage safe for replay after manifest validation.
pub(super) struct ValidatedRegistry {
    pub(super) id: String,
    pub(super) version: String,
    pub(super) digest: Digest,
}

pub(super) fn validate_manifest(manifest: Manifest) -> Result<ValidatedManifest, error::Error> {
    if manifest.format != MANIFEST_FORMAT || manifest.format_version != MANIFEST_VERSION {
        return Err(format!(
            "unsupported capture manifest format/version '{}/{}'; expected '{MANIFEST_FORMAT}/{MANIFEST_VERSION}'",
            manifest.format, manifest.format_version,
        )
        .into());
    }
    if manifest.registry.path != REGISTRY_PATH {
        return Err(format!(
            "capture manifest registry path must be '{REGISTRY_PATH}', found '{}'",
            manifest.registry.path
        )
        .into());
    }
    if manifest.reference.path != REFERENCE_PATH {
        return Err(format!(
            "capture manifest reference path must be '{REFERENCE_PATH}', found '{}'",
            manifest.reference.path
        )
        .into());
    }
    if manifest.component_id.is_empty()
        || manifest.target.name.is_empty()
        || manifest.target.language.is_empty()
        || manifest.tool.name.is_empty()
        || manifest.tool.version.is_empty()
        || manifest.registry.id.is_empty()
        || manifest.registry.version.is_empty()
    {
        return Err("capture manifest identity fields must not be empty".to_string().into());
    }
    let scenario_count =
        i32::try_from(manifest.scenarios.names.len()).map_err(|_error| "capture manifest scenario count exceeds i32".to_string())?;
    if manifest.scenarios.count != scenario_count {
        return Err(format!(
            "capture manifest scenario count {} does not match {} scenario names",
            manifest.scenarios.count, scenario_count
        )
        .into());
    }
    if manifest.scenarios.count <= 0 {
        return Err("capture manifest must declare at least one scenario".to_string().into());
    }
    let unique_names = manifest.scenarios.names.iter().collect::<BTreeSet<_>>();
    if unique_names.len() != manifest.scenarios.names.len() {
        return Err("capture manifest contains duplicate scenario names".to_string().into());
    }
    let mut scenario_names = manifest.scenarios.names;
    scenario_names.shrink_to_fit();
    Ok(ValidatedManifest {
        component_id: manifest.component_id,
        registry: ValidatedRegistry {
            id: manifest.registry.id,
            version: manifest.registry.version,
            digest: manifest.registry.digest,
        },
        reference_digest: manifest.reference.digest,
        scenario_names,
    })
}

/// Syntactically valid lowercase SHA-256 artifact digest.
#[derive(Debug)]
pub(super) struct Digest(String);

impl Digest {
    fn parse(value: impl Into<String>) -> Result<Self, super::Error> {
        // SHA-256 has 32 bytes, rendered as two lowercase hexadecimal characters each.
        const HEX_LEN: usize = 64;
        let value = value.into();
        let valid = value
            .strip_prefix("sha256:")
            .is_some_and(|hex| hex.len() == HEX_LEN && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        if valid {
            Ok(Self(value))
        } else {
            Err("digest must be 'sha256:' followed by 64 lowercase hexadecimal characters"
                .to_string()
                .into())
        }
    }
    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}
impl std::fmt::Display for Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl PartialEq<String> for Digest {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}
