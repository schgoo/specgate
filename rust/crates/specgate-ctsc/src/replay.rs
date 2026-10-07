//! Verified replay planning from linked capture bundles.

mod error;
pub use error::Error;

use crate::registry::Component;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

// CTSC contract version; changing it changes accepted artifact compatibility.
const CTSC_VERSION: &str = "0.2.0";
// Capture manifest discriminator and version from the capture-bundle contract.
const MANIFEST_FORMAT: &str = "specgate.capture-manifest";
const MANIFEST_VERSION: &str = "0.1.0";
// Fixed by the CTSC registry contract; changing it changes artifact compatibility.
const REGISTRY_FORMAT: &str = "ctsc.registry";
/// Stable CTSC registry digest scheme prefix; changing it breaks manifest linkage.
const DIGEST_SCHEME: &str = "sha256:";
// OTLP trace IDs are 16 bytes (32 hex characters) and span IDs are 8 bytes (16 hex characters);
// changing these lengths breaks OTLP interoperability and CTSC linkage.
const TRACE_WIDTH: usize = 32;
const SPAN_WIDTH: usize = 16;
// CTSC Trace Core interoperability keys; changing these spellings breaks capture/replay compatibility.
const RUN_SPAN: &str = "conformance.run";
const SCENARIO_SPAN: &str = "conformance.scenario";
const OPERATION_SPAN: &str = "conformance.operation";
const ERROR_EVENT: &str = "conformance.error";
const REGISTRY_ID: &str = "conformance.registry.id";
const REGISTRY_VERSION: &str = "conformance.registry.version";
const REGISTRY_DIGEST: &str = "conformance.registry.digest";
const SCENARIO_NAME: &str = "conformance.scenario.name";
const SCENARIO_INDEX: &str = "conformance.scenario.index";
const OPERATION_NAME: &str = "conformance.operation.name";
const OPERATION_INPUTS: &str = "conformance.operation.inputs";
const ERROR_NAME: &str = "conformance.error.name";
const ERROR_VALUE: &str = "conformance.error.value";
// Trace Core resource, span, event, and attribute spellings are fixed interoperability keys.
const VERSION_KEY: &str = "conformance.version";
const TOOL_NAME: &str = "conformance.tool.name";
const TOOL_VERSION_KEY: &str = "conformance.tool.version";
const TARGET_NAME: &str = "conformance.target.name";
const TARGET_LANGUAGE: &str = "conformance.target.language";
const PARALLEL_SPAN: &str = "conformance.parallel";
const COMPONENT_ID: &str = "conformance.component.id";
const RESULT_EVENT: &str = "conformance.result";
const RESULT_VALUE: &str = "conformance.result.value";
const OBS_EVENT: &str = "conformance.observation";
const OBS_NAME: &str = "conformance.observation.name";
const FAULT_EVENT: &str = "conformance.fault";
const EMPTY_EVENT: &str = "conformance.empty";
pub mod model;
use model::{
    ArtifactDigest, Bundle, ComponentId, Input, Operation, OperationName, Outcome, Registry, RegistryId, RegistryIdentity, RegistryOp,
    RegistryVersion, Scenario, ScenarioIndex, ScenarioName, Type, Value,
};

/// Decode and link-validate exact manifest, registry, and trace bytes.
///
/// # Examples
/// ```no_run
/// let bundle = specgate_ctsc::replay::decode(
///     std::fs::read("manifest.json")?,
///     std::fs::read("registry.ctsc.json")?,
///     std::fs::read("reference.otlp.json")?,
/// )?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Errors
/// Returns a structured failure for malformed bytes, digest mismatches,
/// unsupported values, or invalid linkage.
pub fn decode(manifest: impl AsRef<[u8]>, registry: impl AsRef<[u8]>, trace: impl AsRef<[u8]>) -> Result<Bundle, Error> {
    decode_inner(manifest.as_ref(), registry.as_ref(), trace.as_ref())
}

/// Decode and link-validate the three exact files in a capture bundle.
///
/// The manifest format/version, exact registry and trace digests, registry
/// identity/version/digest trace attributes, span parentage, scenario order,
/// operation declarations, input names, and supported typed values are checked
/// before any replay scenario is returned. Only top-level operation spans are
/// returned as replay instructions; nested operations are validated but remain
/// observed reference behavior.
///
/// # Errors
///
/// Returns an actionable error for malformed JSON, invalid manifests or
/// digests, broken trace linkage, empty scenarios, or unsupported values.
fn decode_inner(manifest_json: &[u8], registry_json: &[u8], trace_json: &[u8]) -> Result<Bundle, Error> {
    let manifest: Manifest =
        serde_json::from_slice(manifest_json).map_err(|error| Error::json("malformed capture manifest JSON", error))?;
    let manifest = validate_manifest(manifest)?;

    let registry_digest = sha256_digest(registry_json);
    if manifest.registry.digest != registry_digest {
        return Err(format!(
            "capture registry digest mismatch: manifest declares '{}' but exact registry bytes digest to '{registry_digest}'",
            manifest.registry.digest
        )
        .into());
    }
    let reference_digest = sha256_digest(trace_json);
    if manifest.reference_digest != reference_digest {
        return Err(format!(
            "capture reference digest mismatch: manifest declares '{}' but exact reference bytes digest to '{reference_digest}'",
            manifest.reference_digest
        )
        .into());
    }

    let registry_document: RegistryDoc =
        serde_json::from_slice(registry_json).map_err(|error| Error::json("malformed CTSC registry JSON", error))?;
    if registry_document.format != REGISTRY_FORMAT || registry_document.format_version != CTSC_VERSION {
        return Err(format!(
            "unsupported CTSC registry format/version '{}/{}'; expected '{REGISTRY_FORMAT}/{CTSC_VERSION}'",
            registry_document.format, registry_document.format_version,
        )
        .into());
    }
    if registry_document.registry_id != manifest.registry.id {
        return Err(format!(
            "capture manifest registry ID '{}' does not match registry document '{}'",
            manifest.registry.id, registry_document.registry_id
        )
        .into());
    }
    if registry_document.version != manifest.registry.version {
        return Err(format!(
            "capture manifest registry version '{}' does not match registry document '{}'",
            manifest.registry.version, registry_document.version
        )
        .into());
    }

    let registry = decode_registry(&registry_document, registry_digest)?;
    if !registry
        .operations
        .iter()
        .any(|operation| operation.component_id().as_str() == manifest.component_id)
    {
        return Err(format!(
            "capture component '{}' is absent from registry '{}'",
            manifest.component_id, registry.identity.id
        )
        .into());
    }

    let document: OtlpDoc = serde_json::from_slice(trace_json).map_err(|error| Error::json("malformed reference OTLP JSON", error))?;
    let scenarios = decode_scenarios(&document, &manifest, &registry)?;

    Ok(Bundle {
        component_id: ComponentId::try_new(manifest.component_id)?,
        registry,
        scenarios,
    })
}

mod manifest;
use manifest::{Manifest, ValidatedManifest, validate_manifest};

fn sha256_digest(bytes: &[u8]) -> String {
    format!("{DIGEST_SCHEME}{:x}", Sha256::digest(bytes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryDoc {
    format: String,
    format_version: String,
    registry_id: String,
    version: String,
    components: Vec<Component>,
}

fn decode_registry(document: &RegistryDoc, digest: String) -> Result<Registry, Error> {
    let operation_capacity = document.components.iter().map(|component| component.operations.len()).sum();
    let mut component_ids = BTreeSet::new();
    let mut operation_keys = BTreeSet::new();
    let mut input_names = BTreeSet::new();
    let mut operations = Vec::with_capacity(operation_capacity);
    for component in &document.components {
        if component.id.is_empty() || !component_ids.insert(component.id.as_str()) {
            return Err(format!("CTSC registry contains an empty or duplicate component ID '{}'", component.id).into());
        }
        for operation in &component.operations {
            if operation.name.is_empty() || !operation_keys.insert((component.id.as_str(), operation.name.as_str())) {
                return Err(format!(
                    "CTSC registry contains an empty or duplicate operation '{}::{}'",
                    component.id, operation.name
                )
                .into());
            }
            input_names.clear();
            input_names.extend(operation.inputs.iter().map(|input| input.name.as_str()));
            if input_names.len() != operation.inputs.len() {
                return Err(format!(
                    "CTSC registry operation '{}::{}' contains duplicate input names",
                    component.id, operation.name
                )
                .into());
            }
            let mut builder = RegistryOp::builder(model::OpDeps {
                component_id: ComponentId::try_new(component.id.clone())?,
                name: OperationName::try_new(operation.name.clone())?,
                inputs: operation.inputs.clone(),
            });
            if let Some(output) = &operation.outcomes.result {
                builder = builder.output(output.clone());
            }
            if operation.outcomes.empty.unwrap_or(false) {
                builder = builder.empty();
            }
            for error in &operation.outcomes.errors {
                builder = builder.error(Outcome {
                    name: error.name.clone(),
                    value_type: error.value_type.clone(),
                });
            }
            operations.push(builder.build()?);
        }
    }
    if operations.is_empty() {
        return Err("CTSC registry contains no operations".to_string().into());
    }
    operations.shrink_to_fit();
    Ok(Registry {
        identity: RegistryIdentity {
            id: RegistryId::try_new(&document.registry_id)?,
            version: RegistryVersion::try_new(&document.version)?,
            digest: ArtifactDigest::try_new(digest).expect("computed SHA-256 digest is canonical"),
        },
        operations,
    })
}

mod otlp;
use otlp::{Document as OtlpDoc, decode_scenarios};
