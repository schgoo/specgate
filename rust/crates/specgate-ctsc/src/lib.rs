//! CTSC registry, trace, capture-bundle, and replay models for `SpecGate`.
//!
//! Native captures are encoded directly from real annotated operation
//! boundaries, preserving nested parentage, typed inputs/results, observations,
//! empty/error/fault completion, logical timestamps, and deterministic IDs.
//! Registry encoding consumes normalized discovery metadata while retaining
//! setup/dependency/outcome information. The crate also provides native CTSC
//! Registry, Trace Core, Linked, capture-bundle validation, and deterministic
//! `ctsc.strict/0.1.0` differential comparison.

pub mod comparison;
pub mod validation;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specgate_runtime::{NativeCapture, NativeCompletion, NativeOperationSpan, NativeStatus, Value};
use std::collections::{BTreeMap, BTreeSet};

const CTSC_VERSION: &str = "0.2.0";
const CTSC_SCHEMA_URL: &str = "https://specgate.dev/ctsc/schema/0.2.0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CtscNativeCaptureEncoding {
    pub span_count: i32,
    pub otlp_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CtscRegistryEncoding {
    pub operation_count: i32,
    pub type_count: i32,
    pub registry_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayType {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(rename = "componentId", default, skip_serializing_if = "Option::is_none")]
        component_id: Option<String>,
        #[serde(rename = "registryId", default, skip_serializing_if = "Option::is_none")]
        registry_id: Option<String>,
    },
    List {
        items: Box<ReplayType>,
    },
    Set {
        items: Box<ReplayType>,
    },
    Map {
        keys: Box<ReplayType>,
        values: Box<ReplayType>,
    },
    Tuple {
        items: Vec<ReplayType>,
    },
    Optional {
        value: Box<ReplayType>,
    },
    Record {
        fields: Vec<ReplayRegistryInput>,
    },
}

impl ReplayType {
    #[must_use]
    pub fn primitive_name(&self) -> Option<&str> {
        match self {
            Self::Primitive { name } => Some(name),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistryInput {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: ReplayType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistryOperation {
    pub component_id: String,
    pub name: String,
    pub inputs: Vec<ReplayRegistryInput>,
    pub output: Option<ReplayType>,
    pub empty: bool,
    pub errors: Vec<ReplayRegistryError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistryError {
    pub name: String,
    pub value_type: Option<ReplayType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistry {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub operations: Vec<ReplayRegistryOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReplayValue {
    Unit,
    String(String),
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32Bits(u32),
    F64Bits(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayInput {
    pub name: String,
    pub value_type: ReplayType,
    pub value: ReplayValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayOperation {
    pub component_id: String,
    pub operation_name: String,
    pub inputs: Vec<ReplayInput>,
    pub output: Option<ReplayType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayScenario {
    pub name: String,
    pub index: i64,
    pub operations: Vec<ReplayOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayBundle {
    pub component_id: String,
    pub registry: ReplayRegistry,
    pub scenarios: Vec<ReplayScenario>,
}

/// Merge ordered native scenario captures into one deterministic linked CTSC
/// run.
///
/// Capture-local identifiers and logical timestamps are rebased into one
/// non-zero trace/span sequence. Operation parentage, semantic inputs,
/// observations, completions, and scenario order are preserved.
///
/// # Errors
///
/// Returns an error for an empty capture list, malformed native parentage,
/// identifier exhaustion, timestamp overflow, or JSON serialization failure.
#[allow(clippy::too_many_arguments)]
pub fn encode_native_captures_otlp_result(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    encode_native_captures_with_identity(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        "00000000000000000000000000000001",
        1,
        "specgate.capture",
    )
}

/// Encode candidate replay captures as one deterministic run with identity
/// independent from the reference capture encoder.
///
/// # Errors
///
/// Returns the same structural, identifier, timestamp, and serialization
/// errors as [`encode_native_captures_otlp_result`].
#[allow(clippy::too_many_arguments)]
pub fn encode_replayed_native_captures_otlp_result(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    encode_native_captures_with_identity(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        "00000000000000000000000000000002",
        0x8000_0000_0000_0001,
        "specgate.replay",
    )
}

#[allow(clippy::too_many_arguments)]
fn encode_native_captures_with_identity(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
    trace_id: &str,
    first_span_id: u64,
    run_id: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    if captures.is_empty() {
        return Err("cannot encode a CTSC run without captured scenarios".to_string());
    }

    let trace_id = trace_id.to_string();
    let run_span_id = deterministic_span_id(first_span_id)?;
    let mut next_span_id = first_span_id
        .checked_add(1)
        .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
    let mut next_scenario_time = 2_i64;
    let mut scenario_spans = Vec::new();
    let mut operation_count = 0_usize;
    let mut run_has_error = false;

    for (scenario_index, capture) in captures.iter().enumerate() {
        let scenario_span_id = deterministic_span_id(next_span_id)?;
        next_span_id = next_span_id
            .checked_add(1)
            .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;

        let mut operation_ids = BTreeMap::new();
        for operation in &capture.operations {
            let span_id = deterministic_span_id(next_span_id)?;
            next_span_id = next_span_id
                .checked_add(1)
                .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
            if operation_ids.insert(operation.span_id.clone(), span_id).is_some() {
                return Err(format!(
                    "native scenario '{}' contains duplicate operation span ID '{}'",
                    capture.scenario_name, operation.span_id
                ));
            }
        }

        let time_offset = next_scenario_time
            .checked_sub(capture.scenario.start_time_unix_nano)
            .ok_or_else(|| "CTSC scenario timestamp offset overflow".to_string())?;
        let scenario_end_time = offset_time(capture.scenario.end_time_unix_nano, time_offset)?;
        let scenario_index = i64::try_from(scenario_index).map_err(|_error| "CTSC scenario index exceeds i64".to_string())?;
        run_has_error |= capture.scenario.status == NativeStatus::Error;
        scenario_spans.push(Span {
            trace_id: trace_id.clone(),
            id: scenario_span_id.clone(),
            parent_id: Some(run_span_id.clone()),
            name: "conformance.scenario",
            kind: 1,
            start_time_unix_nano: next_scenario_time.to_string(),
            end_time_unix_nano: scenario_end_time.to_string(),
            attributes: vec![
                string_attribute("conformance.scenario.name", capture.scenario_name.clone()),
                integer_attribute("conformance.scenario.index", scenario_index),
            ],
            events: Vec::new(),
            status: Status {
                code: status_code(capture.scenario.status),
            },
        });

        for operation in &capture.operations {
            let span_id = operation_ids
                .get(&operation.span_id)
                .cloned()
                .ok_or_else(|| "native operation span ID mapping was not created".to_string())?;
            let parent_span_id = if operation.parent_span_id == capture.scenario.span_id {
                scenario_span_id.clone()
            } else {
                operation_ids.get(&operation.parent_span_id).cloned().ok_or_else(|| {
                    format!(
                        "native scenario '{}' operation '{}' has unresolved parent span '{}'",
                        capture.scenario_name, operation.operation_name, operation.parent_span_id
                    )
                })?
            };
            let rebased = rebase_native_operation(operation, span_id, parent_span_id, time_offset)?;
            scenario_spans.push(native_operation_span(&trace_id, &rebased));
            operation_count = operation_count
                .checked_add(1)
                .ok_or_else(|| "CTSC operation count overflow".to_string())?;
        }

        next_scenario_time = scenario_end_time
            .checked_add(1)
            .ok_or_else(|| "CTSC logical timestamp overflow".to_string())?;
    }

    let run_end_time = next_scenario_time;
    let mut spans = vec![Span {
        trace_id: trace_id.clone(),
        id: run_span_id,
        parent_id: None,
        name: "conformance.run",
        kind: 1,
        start_time_unix_nano: "1".to_string(),
        end_time_unix_nano: run_end_time.to_string(),
        attributes: vec![string_attribute("conformance.run.id", run_id)],
        events: Vec::new(),
        status: Status {
            code: status_code(if run_has_error { NativeStatus::Error } else { NativeStatus::Ok }),
        },
    }];
    spans.extend(scenario_spans);

    let schema_url = CTSC_SCHEMA_URL.to_string();
    let document = OtlpDocument {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute("conformance.version", CTSC_VERSION),
                    string_attribute("conformance.tool.name", "specgate"),
                    string_attribute("conformance.tool.version", tool_version),
                    string_attribute("conformance.target.name", target_name),
                    string_attribute("conformance.target.language", target_language),
                    string_attribute("conformance.registry.id", registry_id),
                    string_attribute("conformance.registry.version", registry_version),
                    string_attribute("conformance.registry.digest", registry_digest),
                ],
            },
            scope_spans: vec![ScopeSpans {
                scope: InstrumentationScope {
                    name: "specgate.ctsc",
                    version: tool_version.to_string(),
                },
                spans,
                schema_url: schema_url.clone(),
            }],
            schema_url,
        }],
    };
    let otlp_json = serde_json::to_string(&document).map_err(|error| format!("native CTSC OTLP serialization failed: {error}"))?;
    let span_count =
        i32::try_from(operation_count + captures.len() + 1).map_err(|_error| "native CTSC span count exceeds i32".to_string())?;
    Ok(CtscNativeCaptureEncoding { span_count, otlp_json })
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
pub fn decode_replay_bundle_result(manifest_json: &[u8], registry_json: &[u8], reference_otlp_json: &[u8]) -> Result<ReplayBundle, String> {
    let manifest: CaptureManifestWire =
        serde_json::from_slice(manifest_json).map_err(|error| format!("malformed capture manifest JSON: {error}"))?;
    validate_capture_manifest(&manifest)?;

    let registry_digest = replay_sha256_digest(registry_json);
    if manifest.registry.digest != registry_digest {
        return Err(format!(
            "capture registry digest mismatch: manifest declares '{}' but exact registry bytes digest to '{registry_digest}'",
            manifest.registry.digest
        ));
    }
    let reference_digest = replay_sha256_digest(reference_otlp_json);
    if manifest.reference.digest != reference_digest {
        return Err(format!(
            "capture reference digest mismatch: manifest declares '{}' but exact reference bytes digest to '{reference_digest}'",
            manifest.reference.digest
        ));
    }

    let registry_document: ReplayRegistryDocumentWire =
        serde_json::from_slice(registry_json).map_err(|error| format!("malformed CTSC registry JSON: {error}"))?;
    if registry_document.format != "ctsc.registry" || registry_document.format_version != CTSC_VERSION {
        return Err(format!(
            "unsupported CTSC registry format/version '{}/{}'; expected 'ctsc.registry/{CTSC_VERSION}'",
            registry_document.format, registry_document.format_version
        ));
    }
    if registry_document.registry_id != manifest.registry.id {
        return Err(format!(
            "capture manifest registry ID '{}' does not match registry document '{}'",
            manifest.registry.id, registry_document.registry_id
        ));
    }
    if registry_document.version != manifest.registry.version {
        return Err(format!(
            "capture manifest registry version '{}' does not match registry document '{}'",
            manifest.registry.version, registry_document.version
        ));
    }

    let registry = replay_registry(&registry_document, registry_digest)?;
    if !registry
        .operations
        .iter()
        .any(|operation| operation.component_id == manifest.component_id)
    {
        return Err(format!(
            "capture component '{}' is absent from registry '{}'",
            manifest.component_id, registry.id
        ));
    }

    let document: ReplayOtlpDocumentWire =
        serde_json::from_slice(reference_otlp_json).map_err(|error| format!("malformed reference OTLP JSON: {error}"))?;
    let scenarios = decode_replay_scenarios(&document, &manifest, &registry)?;

    Ok(ReplayBundle {
        component_id: manifest.component_id,
        registry,
        scenarios,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureManifestWire {
    format: String,
    format_version: String,
    component_id: String,
    target: CaptureManifestTargetWire,
    tool: CaptureManifestToolWire,
    registry: CaptureManifestRegistryWire,
    reference: CaptureManifestReferenceWire,
    scenarios: CaptureManifestScenariosWire,
}

#[derive(Deserialize)]
struct CaptureManifestTargetWire {
    name: String,
    language: String,
}

#[derive(Deserialize)]
struct CaptureManifestToolWire {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct CaptureManifestRegistryWire {
    path: String,
    id: String,
    version: String,
    digest: String,
}

#[derive(Deserialize)]
struct CaptureManifestReferenceWire {
    path: String,
    digest: String,
}

#[derive(Deserialize)]
struct CaptureManifestScenariosWire {
    count: i32,
    names: Vec<String>,
}

fn validate_capture_manifest(manifest: &CaptureManifestWire) -> Result<(), String> {
    if manifest.format != "specgate.capture-manifest" || manifest.format_version != "0.1.0" {
        return Err(format!(
            "unsupported capture manifest format/version '{}/{}'; expected 'specgate.capture-manifest/0.1.0'",
            manifest.format, manifest.format_version
        ));
    }
    if manifest.registry.path != "registry.ctsc.json" {
        return Err(format!(
            "capture manifest registry path must be 'registry.ctsc.json', found '{}'",
            manifest.registry.path
        ));
    }
    if manifest.reference.path != "reference.otlp.json" {
        return Err(format!(
            "capture manifest reference path must be 'reference.otlp.json', found '{}'",
            manifest.reference.path
        ));
    }
    if manifest.component_id.is_empty()
        || manifest.target.name.is_empty()
        || manifest.target.language.is_empty()
        || manifest.tool.name.is_empty()
        || manifest.tool.version.is_empty()
        || manifest.registry.id.is_empty()
        || manifest.registry.version.is_empty()
    {
        return Err("capture manifest identity fields must not be empty".to_string());
    }
    validate_replay_digest("manifest registry", &manifest.registry.digest)?;
    validate_replay_digest("manifest reference", &manifest.reference.digest)?;
    let scenario_count =
        i32::try_from(manifest.scenarios.names.len()).map_err(|_error| "capture manifest scenario count exceeds i32".to_string())?;
    if manifest.scenarios.count != scenario_count {
        return Err(format!(
            "capture manifest scenario count {} does not match {} scenario names",
            manifest.scenarios.count, scenario_count
        ));
    }
    if manifest.scenarios.count <= 0 {
        return Err("capture manifest must declare at least one scenario".to_string());
    }
    let unique_names = manifest.scenarios.names.iter().collect::<BTreeSet<_>>();
    if unique_names.len() != manifest.scenarios.names.len() {
        return Err("capture manifest contains duplicate scenario names".to_string());
    }
    Ok(())
}

fn validate_replay_digest(label: &str, digest: &str) -> Result<(), String> {
    let valid = digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} digest must be 'sha256:' followed by 64 lowercase hexadecimal characters"
        ))
    }
}

fn replay_sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayRegistryDocumentWire {
    format: String,
    format_version: String,
    registry_id: String,
    version: String,
    components: Vec<RegistryComponent>,
}

fn replay_registry(document: &ReplayRegistryDocumentWire, digest: String) -> Result<ReplayRegistry, String> {
    let mut component_ids = BTreeSet::new();
    let mut operation_keys = BTreeSet::new();
    let mut operations = Vec::new();
    for component in &document.components {
        if component.id.is_empty() || !component_ids.insert(component.id.as_str()) {
            return Err(format!(
                "CTSC registry contains an empty or duplicate component ID '{}'",
                component.id
            ));
        }
        for operation in &component.operations {
            if operation.name.is_empty() || !operation_keys.insert((component.id.as_str(), operation.name.as_str())) {
                return Err(format!(
                    "CTSC registry contains an empty or duplicate operation '{}::{}'",
                    component.id, operation.name
                ));
            }
            let input_names = operation.inputs.iter().map(|input| input.name.as_str()).collect::<BTreeSet<_>>();
            if input_names.len() != operation.inputs.len() {
                return Err(format!(
                    "CTSC registry operation '{}::{}' contains duplicate input names",
                    component.id, operation.name
                ));
            }
            operations.push(ReplayRegistryOperation {
                component_id: component.id.clone(),
                name: operation.name.clone(),
                inputs: operation.inputs.clone(),
                output: operation.outcomes.result.clone(),
                empty: operation.outcomes.empty.unwrap_or(false),
                errors: operation
                    .outcomes
                    .errors
                    .iter()
                    .map(|error| ReplayRegistryError {
                        name: error.name.clone(),
                        value_type: error.value_type.clone(),
                    })
                    .collect(),
            });
        }
    }
    if operations.is_empty() {
        return Err("CTSC registry contains no operations".to_string());
    }
    Ok(ReplayRegistry {
        id: document.registry_id.clone(),
        version: document.version.clone(),
        digest,
        operations,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayOtlpDocumentWire {
    resource_spans: Vec<ReplayResourceSpansWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayResourceSpansWire {
    resource: ReplayResourceWire,
    scope_spans: Vec<ReplayScopeSpansWire>,
}

#[derive(Deserialize)]
struct ReplayResourceWire {
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
}

#[derive(Deserialize)]
struct ReplayScopeSpansWire {
    #[serde(default)]
    spans: Vec<ReplaySpanWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySpanWire {
    trace_id: String,
    #[serde(rename = "spanId")]
    span_id: String,
    #[serde(default, rename = "parentSpanId")]
    parent_span_id: String,
    name: String,
    start_time_unix_nano: String,
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
    #[serde(default)]
    events: Vec<ReplayEventWire>,
}

#[derive(Deserialize)]
struct ReplayEventWire {
    name: String,
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
}

#[derive(Deserialize)]
struct ReplayKeyValueWire {
    key: String,
    value: ReplayAnyValueWire,
}

#[allow(dead_code)]
#[derive(Deserialize)]
enum ReplayAnyValueWire {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(ReplayDoubleWire),
    #[serde(rename = "bytesValue")]
    Bytes(String),
    #[serde(rename = "arrayValue")]
    Array(ReplayArrayWire),
    #[serde(rename = "kvlistValue")]
    Kvlist(ReplayKeyValueListWire),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReplayDoubleWire {
    Number(f64),
    Symbol(String),
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ReplayArrayWire {
    #[serde(default)]
    values: Vec<ReplayAnyValueWire>,
}

#[derive(Deserialize)]
struct ReplayKeyValueListWire {
    #[serde(default)]
    values: Vec<ReplayKeyValueWire>,
}

struct ReplaySpanRef<'a> {
    span: &'a ReplaySpanWire,
    position: usize,
}

struct ReplayScenarioRef<'a> {
    trace_id: &'a str,
    span_id: &'a str,
    name: String,
    index: i64,
    position: usize,
}

fn decode_replay_scenarios(
    document: &ReplayOtlpDocumentWire,
    manifest: &CaptureManifestWire,
    registry: &ReplayRegistry,
) -> Result<Vec<ReplayScenario>, String> {
    if document.resource_spans.len() != 1 {
        return Err(format!(
            "reference OTLP must contain exactly one resourceSpans entry, found {}",
            document.resource_spans.len()
        ));
    }
    let resource = &document.resource_spans[0];
    let resource_attributes = replay_attribute_map(&resource.resource.attributes, "reference resource")?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.version",
        "reference resource",
        Some(CTSC_VERSION),
    )?;
    require_replay_string_attribute(&resource_attributes, "conformance.tool.name", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.tool.version", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.target.name", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.target.language", "reference resource", None)?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.id",
        "reference resource",
        Some(&registry.id),
    )?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.version",
        "reference resource",
        Some(&registry.version),
    )?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.digest",
        "reference resource",
        Some(&registry.digest),
    )?;

    let spans = resource
        .scope_spans
        .iter()
        .flat_map(|scope| scope.spans.iter())
        .enumerate()
        .map(|(position, span)| ReplaySpanRef { span, position })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return Err("reference OTLP contains no spans".to_string());
    }

    let mut by_id = BTreeMap::new();
    for span_ref in &spans {
        validate_replay_hex_id("trace ID", &span_ref.span.trace_id, 32)?;
        validate_replay_hex_id("span ID", &span_ref.span.span_id, 16)?;
        if by_id
            .insert((span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()), span_ref.span)
            .is_some()
        {
            return Err(format!(
                "reference OTLP contains duplicate span ID '{}' in trace '{}'",
                span_ref.span.span_id, span_ref.span.trace_id
            ));
        }
        parse_replay_time(&span_ref.span.start_time_unix_nano, &format!("span '{}'", span_ref.span.span_id))?;
    }

    let runs = spans
        .iter()
        .filter(|span_ref| span_ref.span.name == "conformance.run")
        .collect::<Vec<_>>();
    if runs.len() != 1 || !runs[0].span.parent_span_id.is_empty() {
        return Err(format!(
            "reference OTLP must contain exactly one root conformance.run span, found {}",
            runs.len()
        ));
    }

    let mut scenario_refs = Vec::new();
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name == "conformance.scenario") {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_str(), span_ref.span.parent_span_id.as_str()))
            .ok_or_else(|| format!("scenario span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name != "conformance.run" {
            return Err(format!("scenario span '{}' parent is not conformance.run", span_ref.span.span_id));
        }
        let attributes = replay_attribute_map(&span_ref.span.attributes, &format!("scenario span '{}'", span_ref.span.span_id))?;
        let name = require_replay_string_attribute(
            &attributes,
            "conformance.scenario.name",
            &format!("scenario span '{}'", span_ref.span.span_id),
            None,
        )?
        .to_string();
        let index = require_replay_integer_attribute(
            &attributes,
            "conformance.scenario.index",
            &format!("scenario span '{}'", span_ref.span.span_id),
        )?;
        if index < 0 {
            return Err(format!("scenario '{name}' has negative index {index}"));
        }
        scenario_refs.push(ReplayScenarioRef {
            trace_id: &span_ref.span.trace_id,
            span_id: &span_ref.span.span_id,
            name,
            index,
            position: span_ref.position,
        });
    }
    scenario_refs.sort_by_key(|scenario| (scenario.index, scenario.position));
    for (expected, scenario) in scenario_refs.iter().enumerate() {
        let expected = i64::try_from(expected).map_err(|_error| "reference scenario index exceeds i64".to_string())?;
        if scenario.index != expected {
            return Err(format!(
                "reference scenario indexes must be unique and contiguous from zero; expected {expected}, found {}",
                scenario.index
            ));
        }
    }
    let trace_names = scenario_refs.iter().map(|scenario| scenario.name.as_str()).collect::<Vec<_>>();
    let manifest_names = manifest.scenarios.names.iter().map(String::as_str).collect::<Vec<_>>();
    if trace_names != manifest_names {
        return Err(format!(
            "reference scenario order/names {trace_names:?} do not match capture manifest {manifest_names:?}"
        ));
    }

    let mut decoded_operations = BTreeMap::new();
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name == "conformance.operation") {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_str(), span_ref.span.parent_span_id.as_str()))
            .ok_or_else(|| format!("operation span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name != "conformance.scenario" && parent.name != "conformance.operation" {
            return Err(format!(
                "operation span '{}' parent '{}' is not a scenario or operation",
                span_ref.span.span_id, parent.name
            ));
        }
        decoded_operations.insert(
            (span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()),
            decode_replay_operation(span_ref.span, registry)?,
        );
    }
    if spans.iter().any(|span_ref| span_ref.span.name == "conformance.parallel") {
        return Err("reference OTLP parallel spans are unsupported by the first replay slice".to_string());
    }

    let mut scenarios = Vec::new();
    for scenario in scenario_refs {
        let mut top_level = spans
            .iter()
            .filter(|span_ref| {
                span_ref.span.name == "conformance.operation"
                    && span_ref.span.trace_id == scenario.trace_id
                    && span_ref.span.parent_span_id == scenario.span_id
            })
            .collect::<Vec<_>>();
        top_level.sort_by_key(|span_ref| {
            (
                parse_replay_time(&span_ref.span.start_time_unix_nano, "top-level operation").unwrap_or(i64::MAX),
                span_ref.position,
            )
        });
        if top_level.is_empty() {
            return Err(format!("reference scenario '{}' contains no top-level operations", scenario.name));
        }
        let operations = top_level
            .into_iter()
            .map(|span_ref| {
                decoded_operations
                    .get(&(span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()))
                    .cloned()
                    .ok_or_else(|| format!("operation span '{}' was not decoded", span_ref.span.span_id))
            })
            .collect::<Result<Vec<_>, String>>()?;
        scenarios.push(ReplayScenario {
            name: scenario.name,
            index: scenario.index,
            operations,
        });
    }
    Ok(scenarios)
}

fn decode_replay_operation(span: &ReplaySpanWire, registry: &ReplayRegistry) -> Result<ReplayOperation, String> {
    let location = format!("operation span '{}'", span.span_id);
    let attributes = replay_attribute_map(&span.attributes, &location)?;
    let component_id = require_replay_string_attribute(&attributes, "conformance.component.id", &location, None)?.to_string();
    let operation_name = require_replay_string_attribute(&attributes, "conformance.operation.name", &location, None)?.to_string();
    let declaration = registry
        .operations
        .iter()
        .find(|operation| operation.component_id == component_id && operation.name == operation_name)
        .ok_or_else(|| format!("reference {location} names unknown operation '{component_id}::{operation_name}'"))?;
    let input_value = attributes
        .get("conformance.operation.inputs")
        .ok_or_else(|| format!("reference {location} is missing conformance.operation.inputs"))?;
    let ReplayAnyValueWire::Kvlist(input_list) = input_value else {
        return Err(format!("reference {location} operation inputs must use kvlistValue"));
    };
    let input_values = replay_kvlist_map(input_list, &format!("{location} inputs"))?;
    if input_values.len() != declaration.inputs.len() {
        return Err(format!(
            "reference {location} input names do not match registry operation '{}::{}'",
            declaration.component_id, declaration.name
        ));
    }
    let mut inputs = Vec::new();
    for input in &declaration.inputs {
        let value = input_values.get(input.name.as_str()).ok_or_else(|| {
            format!(
                "reference {location} is missing declared input '{}' for '{}::{}'",
                input.name, declaration.component_id, declaration.name
            )
        })?;
        inputs.push(ReplayInput {
            name: input.name.clone(),
            value_type: input.value_type.clone(),
            value: decode_replay_value(value, &input.value_type, &format!("{location} input '{}'", input.name))?,
        });
    }
    for actual in input_values.keys() {
        if !declaration.inputs.iter().any(|input| input.name == *actual) {
            return Err(format!(
                "reference {location} contains undeclared input '{actual}' for '{}::{}'",
                declaration.component_id, declaration.name
            ));
        }
    }

    let mut result_count = 0_usize;
    let mut empty_count = 0_usize;
    let mut error_count = 0_usize;
    let mut has_fault = false;
    for event in &span.events {
        match event.name.as_str() {
            "conformance.result" => {
                result_count += 1;
                let output = declaration.output.as_ref().ok_or_else(|| {
                    format!(
                        "reference {location} emits a result but registry operation '{}::{}' declares no result",
                        declaration.component_id, declaration.name
                    )
                })?;
                let event_attributes = replay_attribute_map(&event.attributes, &format!("{location} result"))?;
                let value = event_attributes
                    .get("conformance.result.value")
                    .ok_or_else(|| format!("reference {location} result is missing conformance.result.value"))?;
                let _ = decode_replay_value(value, output, &format!("{location} result"))?;
            }
            "conformance.observation" => {
                let event_attributes = replay_attribute_map(&event.attributes, &format!("{location} observation"))?;
                let name = require_replay_string_attribute(
                    &event_attributes,
                    "conformance.observation.name",
                    &format!("{location} observation"),
                    None,
                )?;
                return Err(format!(
                    "reference {location} contains unsupported observation '{name}' in the first replay slice"
                ));
            }
            "conformance.fault" => has_fault = true,
            "conformance.empty" => {
                empty_count += 1;
                if !declaration.empty {
                    return Err(format!(
                        "reference {location} emits empty but registry operation '{}::{}' does not declare it",
                        declaration.component_id, declaration.name
                    ));
                }
            }
            "conformance.error" => {
                error_count += 1;
                let attributes = replay_attribute_map(&event.attributes, &format!("{location} error"))?;
                let name = require_replay_string_attribute(&attributes, "conformance.error.name", &format!("{location} error"), None)?;
                let declared = declaration.errors.iter().find(|error| error.name == name).ok_or_else(|| {
                    format!(
                        "reference {location} emits undeclared error '{name}' for '{}::{}'",
                        declaration.component_id, declaration.name
                    )
                })?;
                match (&declared.value_type, attributes.get("conformance.error.value")) {
                    (Some(value_type), Some(value)) => {
                        let _ = decode_replay_value(value, value_type, &format!("{location} error '{name}'"))?;
                    }
                    (Some(_), None) => return Err(format!("reference {location} error '{name}' is missing its value")),
                    (None, Some(_)) => return Err(format!("reference {location} error '{name}' declares no value")),
                    (None, None) => {}
                }
            }
            other => return Err(format!("reference {location} contains unsupported CTSC event '{other}'")),
        }
    }
    let terminal_count = result_count + empty_count + error_count + usize::from(has_fault);
    if terminal_count > 1 {
        return Err(format!("reference {location} contains multiple terminal events"));
    }
    if declaration.output.is_some() && terminal_count == 0 {
        return Err(format!(
            "reference {location} has no result or fault for result-bearing operation '{}::{}'",
            declaration.component_id, declaration.name
        ));
    }

    Ok(ReplayOperation {
        component_id,
        operation_name,
        inputs,
        output: declaration.output.clone(),
    })
}

fn replay_attribute_map<'a>(
    attributes: &'a [ReplayKeyValueWire],
    location: &str,
) -> Result<BTreeMap<&'a str, &'a ReplayAnyValueWire>, String> {
    let mut result = BTreeMap::new();
    for attribute in attributes {
        if result.insert(attribute.key.as_str(), &attribute.value).is_some() {
            return Err(format!("{location} contains duplicate attribute '{}'", attribute.key));
        }
    }
    Ok(result)
}

fn replay_kvlist_map<'a>(list: &'a ReplayKeyValueListWire, location: &str) -> Result<BTreeMap<&'a str, &'a ReplayAnyValueWire>, String> {
    let mut result = BTreeMap::new();
    for entry in &list.values {
        if result.insert(entry.key.as_str(), &entry.value).is_some() {
            return Err(format!("{location} contains duplicate key '{}'", entry.key));
        }
    }
    Ok(result)
}

fn require_replay_string_attribute<'a>(
    attributes: &'a BTreeMap<&str, &'a ReplayAnyValueWire>,
    key: &str,
    location: &str,
    expected: Option<&str>,
) -> Result<&'a str, String> {
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing string attribute '{key}'"))?;
    let ReplayAnyValueWire::String(value) = value else {
        return Err(format!("{location} attribute '{key}' must use stringValue"));
    };
    if let Some(expected) = expected
        && value != expected
    {
        return Err(format!("{location} attribute '{key}' is '{value}', expected '{expected}'"));
    }
    Ok(value)
}

fn require_replay_integer_attribute(attributes: &BTreeMap<&str, &ReplayAnyValueWire>, key: &str, location: &str) -> Result<i64, String> {
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing integer attribute '{key}'"))?;
    let ReplayAnyValueWire::Int(value) = value else {
        return Err(format!("{location} attribute '{key}' must use intValue"));
    };
    value
        .parse::<i64>()
        .map_err(|error| format!("{location} attribute '{key}' is not an i64 decimal integer: {error}"))
}

fn validate_replay_hex_id(label: &str, value: &str, length: usize) -> Result<(), String> {
    if value.len() != length
        || value.bytes().all(|byte| byte == b'0')
        || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "reference {label} '{value}' must be {length} lowercase hexadecimal characters and nonzero"
        ));
    }
    Ok(())
}

fn parse_replay_time(value: &str, location: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<i64>()
        .map_err(|error| format!("{location} startTimeUnixNano is not an i64 decimal integer: {error}"))?;
    if parsed < 0 {
        return Err(format!("{location} startTimeUnixNano must be non-negative"));
    }
    Ok(parsed)
}

fn decode_replay_value(value: &ReplayAnyValueWire, value_type: &ReplayType, location: &str) -> Result<ReplayValue, String> {
    let ReplayType::Primitive { name } = value_type else {
        return Err(format!(
            "{location} uses unsupported structured replay type {}",
            replay_type_name(value_type)
        ));
    };
    match name.as_str() {
        "unit" => {
            let ReplayAnyValueWire::Kvlist(list) = value else {
                return Err(format!("{location} unit value must use kvlistValue"));
            };
            if !list.values.is_empty() {
                return Err(format!("{location} unit value must be an empty kvlistValue"));
            }
            Ok(ReplayValue::Unit)
        }
        "string" => match value {
            ReplayAnyValueWire::String(value) => Ok(ReplayValue::String(value.clone())),
            _ => Err(format!("{location} string value must use stringValue")),
        },
        "bool" => match value {
            ReplayAnyValueWire::Bool(value) => Ok(ReplayValue::Bool(*value)),
            _ => Err(format!("{location} bool value must use boolValue")),
        },
        "i32" => decode_replay_integer(value, location, i64::from(i32::MIN), i64::from(i32::MAX))
            .and_then(|value| i32::try_from(value).map(ReplayValue::I32).map_err(|error| error.to_string())),
        "i64" => decode_replay_integer(value, location, i64::MIN, i64::MAX).map(ReplayValue::I64),
        "u32" => decode_replay_integer(value, location, 0, i64::from(u32::MAX))
            .and_then(|value| u32::try_from(value).map(ReplayValue::U32).map_err(|error| error.to_string())),
        "u64" => {
            let ReplayAnyValueWire::String(value) = value else {
                return Err(format!("{location} u64 value must use stringValue"));
            };
            let parsed = value
                .parse::<u64>()
                .map_err(|error| format!("{location} is not a valid u64 decimal string: {error}"))?;
            if parsed.to_string() != *value {
                return Err(format!("{location} u64 value must use canonical unsigned decimal text"));
            }
            Ok(ReplayValue::U64(parsed))
        }
        "f32" => decode_replay_float(value, location, true),
        "f64" => decode_replay_float(value, location, false),
        "bytes" => Err(format!("{location} uses unsupported replay primitive 'bytes'")),
        other => Err(format!("{location} uses unknown CTSC primitive '{other}'")),
    }
}

fn decode_replay_integer(value: &ReplayAnyValueWire, location: &str, minimum: i64, maximum: i64) -> Result<i64, String> {
    let ReplayAnyValueWire::Int(value) = value else {
        return Err(format!("{location} integer value must use intValue"));
    };
    let parsed = value
        .parse::<i64>()
        .map_err(|error| format!("{location} is not a signed decimal integer: {error}"))?;
    if parsed < minimum || parsed > maximum {
        return Err(format!(
            "{location} integer value {parsed} is outside supported range {minimum}..={maximum}"
        ));
    }
    Ok(parsed)
}

fn decode_replay_float(value: &ReplayAnyValueWire, location: &str, narrow: bool) -> Result<ReplayValue, String> {
    let ReplayAnyValueWire::Double(value) = value else {
        return Err(format!("{location} floating-point value must use doubleValue"));
    };
    match value {
        ReplayDoubleWire::Number(value) if narrow => {
            let narrowed = value
                .to_string()
                .parse::<f32>()
                .map_err(|error| format!("{location} value is outside f32 range: {error}"))?;
            if f64::from(narrowed).to_bits() != value.to_bits() {
                return Err(format!("{location} value is not exactly representable as f32"));
            }
            Ok(ReplayValue::F32Bits(narrowed.to_bits()))
        }
        ReplayDoubleWire::Number(value) => Ok(ReplayValue::F64Bits(value.to_bits())),
        ReplayDoubleWire::Symbol(symbol) => {
            let bits = match (narrow, symbol.as_str()) {
                (true, "NaN") => return Ok(ReplayValue::F32Bits(f32::NAN.to_bits())),
                (true, "Infinity") => return Ok(ReplayValue::F32Bits(f32::INFINITY.to_bits())),
                (true, "-Infinity") => return Ok(ReplayValue::F32Bits(f32::NEG_INFINITY.to_bits())),
                (false, "NaN") => f64::NAN.to_bits(),
                (false, "Infinity") => f64::INFINITY.to_bits(),
                (false, "-Infinity") => f64::NEG_INFINITY.to_bits(),
                _ => return Err(format!("{location} contains invalid symbolic doubleValue '{symbol}'")),
            };
            Ok(ReplayValue::F64Bits(bits))
        }
    }
}

fn replay_type_name(value_type: &ReplayType) -> &'static str {
    match value_type {
        ReplayType::Primitive { .. } => "primitive",
        ReplayType::Named { .. } => "named",
        ReplayType::List { .. } => "list",
        ReplayType::Set { .. } => "set",
        ReplayType::Map { .. } => "map",
        ReplayType::Tuple { .. } => "tuple",
        ReplayType::Optional { .. } => "optional",
        ReplayType::Record { .. } => "record",
    }
}

fn deterministic_span_id(value: u64) -> Result<String, String> {
    if value == 0 {
        return Err("CTSC span IDs must be non-zero".to_string());
    }
    Ok(format!("{value:016x}"))
}

fn offset_time(value: i64, offset: i64) -> Result<i64, String> {
    value
        .checked_add(offset)
        .ok_or_else(|| "CTSC logical timestamp overflow".to_string())
}

fn rebase_native_operation(
    operation: &NativeOperationSpan,
    span_id: String,
    parent_span_id: String,
    time_offset: i64,
) -> Result<NativeOperationSpan, String> {
    let observations = operation
        .observations
        .iter()
        .map(|observation| {
            let mut observation = observation.clone();
            observation.time_unix_nano = offset_time(observation.time_unix_nano, time_offset)?;
            Ok(observation)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let completion = operation
        .completion
        .as_ref()
        .map(|completion| match completion {
            NativeCompletion::Result {
                order,
                time_unix_nano,
                value,
            } => Ok::<NativeCompletion, String>(NativeCompletion::Result {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
                value: value.clone(),
            }),
            NativeCompletion::Empty { order, time_unix_nano } => Ok::<NativeCompletion, String>(NativeCompletion::Empty {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
            }),
            NativeCompletion::Error {
                order,
                time_unix_nano,
                name,
                value,
            } => Ok::<NativeCompletion, String>(NativeCompletion::Error {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
                name: name.clone(),
                value: value.clone(),
            }),
            NativeCompletion::Fault {
                order,
                time_unix_nano,
                fault_type,
                message,
                observer,
            } => Ok::<NativeCompletion, String>(NativeCompletion::Fault {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
                fault_type: fault_type.clone(),
                message: message.clone(),
                observer: observer.clone(),
            }),
        })
        .transpose()?;
    Ok(NativeOperationSpan {
        order: operation.order,
        span_id,
        parent_span_id,
        component_id: operation.component_id.clone(),
        operation_name: operation.operation_name.clone(),
        start_time_unix_nano: offset_time(operation.start_time_unix_nano, time_offset)?,
        end_time_unix_nano: offset_time(operation.end_time_unix_nano, time_offset)?,
        status: operation.status,
        inputs: operation.inputs.clone(),
        observations,
        completion,
    })
}

fn native_operation_span(trace_id: &str, operation: &NativeOperationSpan) -> Span {
    let mut ordered_events = operation
        .observations
        .iter()
        .map(|observation| {
            (
                observation.order,
                SpanEvent {
                    time_unix_nano: observation.time_unix_nano.to_string(),
                    name: "conformance.observation",
                    attributes: vec![
                        string_attribute("conformance.observation.name", observation.name.clone()),
                        KeyValue {
                            key: "conformance.observation.value".to_string(),
                            value: value_to_any_value(&observation.value),
                        },
                    ],
                },
            )
        })
        .collect::<Vec<_>>();
    if let Some(completion) = &operation.completion {
        ordered_events.push(match completion {
            NativeCompletion::Result {
                order,
                time_unix_nano,
                value,
            } => (
                *order,
                SpanEvent {
                    time_unix_nano: time_unix_nano.to_string(),
                    name: "conformance.result",
                    attributes: vec![KeyValue {
                        key: "conformance.result.value".to_string(),
                        value: value_to_any_value(value),
                    }],
                },
            ),
            NativeCompletion::Empty { order, time_unix_nano } => (
                *order,
                SpanEvent {
                    time_unix_nano: time_unix_nano.to_string(),
                    name: "conformance.empty",
                    attributes: Vec::new(),
                },
            ),
            NativeCompletion::Error {
                order,
                time_unix_nano,
                name,
                value,
            } => {
                let mut attributes = vec![string_attribute("conformance.error.name", name.clone())];
                if let Some(value) = value {
                    attributes.push(KeyValue {
                        key: "conformance.error.value".to_string(),
                        value: value_to_any_value(value),
                    });
                }
                (
                    *order,
                    SpanEvent {
                        time_unix_nano: time_unix_nano.to_string(),
                        name: "conformance.error",
                        attributes,
                    },
                )
            }
            NativeCompletion::Fault {
                order,
                time_unix_nano,
                fault_type,
                message,
                observer,
            } => (
                *order,
                SpanEvent {
                    time_unix_nano: time_unix_nano.to_string(),
                    name: "conformance.fault",
                    attributes: vec![
                        string_attribute("conformance.fault.type", fault_type.clone()),
                        string_attribute("conformance.fault.message", message.clone()),
                        string_attribute("conformance.fault.observer", observer.clone()),
                    ],
                },
            ),
        });
    }
    ordered_events.sort_by_key(|(order, _event)| *order);

    Span {
        trace_id: trace_id.to_string(),
        id: operation.span_id.clone(),
        parent_id: Some(operation.parent_span_id.clone()),
        name: "conformance.operation",
        kind: 1,
        start_time_unix_nano: operation.start_time_unix_nano.to_string(),
        end_time_unix_nano: operation.end_time_unix_nano.to_string(),
        attributes: vec![
            string_attribute("conformance.component.id", operation.component_id.clone()),
            string_attribute("conformance.operation.name", operation.operation_name.clone()),
            KeyValue {
                key: "conformance.operation.inputs".to_string(),
                value: AnyValue::Kvlist(KeyValueList {
                    values: operation
                        .inputs
                        .iter()
                        .map(|(key, value)| KeyValue {
                            key: key.clone(),
                            value: value_to_any_value(value),
                        })
                        .collect(),
                }),
            },
        ],
        events: ordered_events.into_iter().map(|(_order, event)| event).collect(),
        status: Status {
            code: status_code(operation.status),
        },
    }
}

const fn status_code(status: NativeStatus) -> i32 {
    match status {
        NativeStatus::Ok => 1,
        NativeStatus::Error => 2,
    }
}

/// Encode one normalized, setup-folded `SpecGate` schema without panicking.
///
/// # Errors
///
/// Returns an error when the schema JSON is malformed, a named type
/// declaration is unsupported, or a type reference is malformed, unsupported,
/// or names a type absent from the schema.
pub fn encode_schema_registry_result(
    registry_id: String,
    registry_version: String,
    schema_json: &str,
) -> Result<CtscRegistryEncoding, String> {
    let schema: NormalizedSchema =
        serde_json::from_str(schema_json).map_err(|error| format!("malformed normalized schema JSON: {error}"))?;
    let mut types_by_component = BTreeMap::new();
    insert_component_type_names(&mut types_by_component, &schema.component, &schema.types)?;
    for dependency in &schema.dependency_types {
        insert_component_type_names(&mut types_by_component, &dependency.component, &dependency.types)?;
    }
    let dependency_names = schema.dependencies.iter().cloned().collect::<BTreeSet<_>>();
    if dependency_names.len() != schema.dependencies.len() {
        return Err("normalized schema contains duplicate component dependencies".to_string());
    }
    let dependency_type_owners = schema
        .dependency_types
        .iter()
        .map(|dependency| dependency.component.as_str())
        .collect::<BTreeSet<_>>();
    if dependency_type_owners.len() != schema.dependency_types.len() {
        return Err("normalized schema contains duplicate dependency type components".to_string());
    }
    if let Some(missing) = dependency_names
        .iter()
        .find(|dependency| !dependency_type_owners.contains(dependency.as_str()))
    {
        return Err(format!("normalized schema dependency '{missing}' has no dependency type component"));
    }
    let all_components = dependency_type_owners
        .iter()
        .copied()
        .chain(std::iter::once(schema.component.as_str()))
        .collect::<BTreeSet<_>>();
    for dependency in &schema.dependency_types {
        if let Some(missing) = dependency
            .dependencies
            .iter()
            .find(|required| !all_components.contains(required.as_str()))
        {
            return Err(format!(
                "normalized dependency component '{}' references missing dependency '{missing}'",
                dependency.component
            ));
        }
    }
    validate_normalized_dependency_cycles(&schema)?;
    let context = TypeContext {
        component: &schema.component,
        dependencies: &dependency_names,
        types_by_component: &types_by_component,
    };

    let mut operations = schema
        .operations
        .iter()
        .map(|operation| operation.to_ctsc(&context))
        .collect::<Result<Vec<_>, _>>()?;
    operations.sort_by(|left, right| left.name.cmp(&right.name));

    let mut types = schema.types.iter().map(|ty| ty.to_ctsc(&context)).collect::<Result<Vec<_>, _>>()?;
    types.sort_by(|left, right| left.name.cmp(&right.name));

    let mut components = vec![RegistryComponent {
        id: schema.component.clone(),
        dependencies: schema
            .dependencies
            .iter()
            .cloned()
            .map(|component_id| RegistryComponentRef { component_id })
            .collect(),
        operations,
        types,
    }];
    let mut dependencies = schema.dependency_types.iter().collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.component.cmp(&right.component));
    for dependency in dependencies {
        let dependency_names = dependency.dependencies.iter().cloned().collect::<BTreeSet<_>>();
        let dependency_context = TypeContext {
            component: &dependency.component,
            dependencies: &dependency_names,
            types_by_component: &types_by_component,
        };
        let mut dependency_types = dependency
            .types
            .iter()
            .map(|ty| ty.to_ctsc(&dependency_context))
            .collect::<Result<Vec<_>, _>>()?;
        dependency_types.sort_by(|left, right| left.name.cmp(&right.name));
        components.push(RegistryComponent {
            id: dependency.component.clone(),
            dependencies: dependency
                .dependencies
                .iter()
                .cloned()
                .map(|component_id| RegistryComponentRef { component_id })
                .collect(),
            operations: Vec::new(),
            types: dependency_types,
        });
    }
    encode_registry_document(registry_id, registry_version, components)
}

/// Encode several normalized component schemas as one deterministic registry.
///
/// Components contributed as dependency type closures are merged with their
/// full selected schema when both are present. Output is byte-identical to
/// [`encode_schema_registry_result`] for a single schema.
///
/// # Errors
///
/// Returns the same errors as [`encode_schema_registry_result`], or an error
/// when repeated component declarations disagree.
pub fn encode_schema_registries_result(
    registry_id: String,
    registry_version: String,
    schema_json: &[String],
) -> Result<CtscRegistryEncoding, String> {
    if schema_json.is_empty() {
        return Err("cannot encode a registry without normalized schemas".to_string());
    }
    let mut components = BTreeMap::<String, RegistryComponent>::new();
    for schema in schema_json {
        let encoded = encode_schema_registry_result(registry_id.clone(), registry_version.clone(), schema)?;
        let document: RegistryDocumentWire =
            serde_json::from_str(&encoded.registry_json).map_err(|error| format!("failed to merge encoded registry JSON: {error}"))?;
        for component in document.components {
            if let Some(existing) = components.get_mut(&component.id) {
                merge_registry_component(existing, component)?;
            } else {
                components.insert(component.id.clone(), component);
            }
        }
    }
    encode_registry_document(registry_id, registry_version, components.into_values().collect())
}

/// Serialize registry components as one deterministic CTSC registry document.
///
/// This is the single ordering rule for every registry this crate emits:
/// components are sorted by ascending component id, with no privileged
/// position for any selected or root component. `discover` and `capture`
/// therefore emit byte-identical documents for the same logical component set,
/// which matters because both write to the same `registry.ctsc.json` path.
fn encode_registry_document(
    registry_id: String,
    registry_version: String,
    mut components: Vec<RegistryComponent>,
) -> Result<CtscRegistryEncoding, String> {
    components.sort_by(|left, right| left.id.cmp(&right.id));
    let operation_count = components.iter().try_fold(0_usize, |count, component| {
        count
            .checked_add(component.operations.len())
            .ok_or_else(|| "operation count overflow".to_string())
    })?;
    let type_count = components.iter().try_fold(0_usize, |count, component| {
        count
            .checked_add(component.types.len())
            .ok_or_else(|| "type count overflow".to_string())
    })?;
    let document = RegistryDocument {
        format: "ctsc.registry",
        format_version: CTSC_VERSION,
        registry_id,
        version: registry_version,
        components,
    };
    Ok(CtscRegistryEncoding {
        operation_count: i32::try_from(operation_count).map_err(|_error| "operation count exceeds i32".to_string())?,
        type_count: i32::try_from(type_count).map_err(|_error| "type count exceeds i32".to_string())?,
        registry_json: serde_json::to_string(&document).map_err(|error| format!("registry JSON serialization failed: {error}"))?,
    })
}

fn merge_registry_component(existing: &mut RegistryComponent, incoming: RegistryComponent) -> Result<(), String> {
    let dependencies = existing
        .dependencies
        .iter()
        .chain(&incoming.dependencies)
        .map(|dependency| dependency.component_id.clone())
        .collect::<BTreeSet<_>>();
    existing.dependencies = dependencies
        .into_iter()
        .map(|component_id| RegistryComponentRef { component_id })
        .collect();

    for operation in incoming.operations {
        match existing.operations.iter().find(|candidate| candidate.name == operation.name) {
            Some(candidate) if candidate != &operation => {
                return Err(format!(
                    "normalized schemas disagree on operation '{}::{}'",
                    existing.id, operation.name
                ));
            }
            Some(_) => {}
            None => existing.operations.push(operation),
        }
    }
    existing.operations.sort_by(|left, right| left.name.cmp(&right.name));
    for ty in incoming.types {
        match existing.types.iter().find(|candidate| candidate.name == ty.name) {
            Some(candidate) if candidate != &ty => {
                return Err(format!("normalized schemas disagree on type '{}::{}'", existing.id, ty.name));
            }
            Some(_) => {}
            None => existing.types.push(ty),
        }
    }
    existing.types.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(())
}

#[derive(Deserialize)]
struct NormalizedSchema {
    component: String,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    dependency_types: Vec<NormalizedDependencyTypes>,
    operations: Vec<NormalizedOperation>,
    types: Vec<NormalizedType>,
}

#[derive(Deserialize)]
struct NormalizedDependencyTypes {
    component: String,
    #[serde(default)]
    dependencies: Vec<String>,
    types: Vec<NormalizedType>,
}

struct TypeContext<'a> {
    component: &'a str,
    dependencies: &'a BTreeSet<String>,
    types_by_component: &'a BTreeMap<String, BTreeSet<String>>,
}

fn insert_component_type_names(
    types_by_component: &mut BTreeMap<String, BTreeSet<String>>,
    component: &str,
    types: &[NormalizedType],
) -> Result<(), String> {
    if types_by_component.contains_key(component) {
        return Err(format!("normalized schema repeats component type owner '{component}'"));
    }
    let names = types.iter().map(|ty| ty.name.clone()).collect::<BTreeSet<_>>();
    if names.len() != types.len() {
        return Err(format!("normalized component '{component}' contains duplicate type names"));
    }
    types_by_component.insert(component.to_string(), names);
    Ok(())
}

fn validate_normalized_dependency_cycles(schema: &NormalizedSchema) -> Result<(), String> {
    let mut graph = schema
        .dependency_types
        .iter()
        .map(|dependency| (dependency.component.as_str(), dependency.dependencies.as_slice()))
        .collect::<BTreeMap<_, _>>();
    graph.insert(schema.component.as_str(), schema.dependencies.as_slice());
    let mut complete = BTreeSet::new();
    let mut visiting = Vec::new();
    visit_normalized_dependency(&schema.component, &graph, &mut complete, &mut visiting)?;
    if complete.len() != graph.len() {
        let unreachable = graph
            .keys()
            .filter(|component| !complete.contains(**component))
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "normalized schema contains unreachable dependency components: {unreachable}"
        ));
    }
    Ok(())
}

fn visit_normalized_dependency<'a>(
    component: &'a str,
    graph: &BTreeMap<&'a str, &'a [String]>,
    complete: &mut BTreeSet<&'a str>,
    visiting: &mut Vec<&'a str>,
) -> Result<(), String> {
    if complete.contains(component) {
        return Ok(());
    }
    if let Some(position) = visiting.iter().position(|candidate| *candidate == component) {
        let mut cycle = visiting[position..].to_vec();
        cycle.push(component);
        return Err(format!("component dependency cycle: {}", cycle.join(" -> ")));
    }
    visiting.push(component);
    for dependency in graph.get(component).copied().unwrap_or_default() {
        visit_normalized_dependency(dependency, graph, complete, visiting)?;
    }
    visiting.pop();
    complete.insert(component);
    Ok(())
}

#[derive(Deserialize)]
struct NormalizedOperation {
    name: String,
    #[serde(rename = "is_async")]
    _is_async: bool,
    inputs: Vec<NormalizedInput>,
    output: String,
    #[serde(default)]
    empty: bool,
    #[serde(default)]
    errors: Vec<NormalizedError>,
    #[serde(default)]
    setups: Vec<NormalizedSetup>,
}

impl NormalizedOperation {
    fn to_ctsc(&self, context: &TypeContext<'_>) -> Result<RegistryOperation, String> {
        let inputs = self
            .inputs
            .iter()
            .map(|input| {
                Ok(NamedValue {
                    name: input.name.clone(),
                    value_type: schema_type_ref(&input.ty, context)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let result = if is_schema_unit_type(&self.output) {
            None
        } else {
            Some(schema_type_ref(&self.output, context)?)
        };
        let errors = self
            .errors
            .iter()
            .map(|error| {
                Ok(RegistryErrorOutcome {
                    name: error.name.clone(),
                    value_type: if is_schema_unit_type(&error.ty) {
                        None
                    } else {
                        Some(schema_type_ref(&error.ty, context)?)
                    },
                })
            })
            .collect::<Result<Vec<_>, String>>()?;

        Ok(RegistryOperation {
            name: self.name.clone(),
            inputs,
            observations: Vec::new(),
            outcomes: RegistryOutcomes {
                result,
                empty: self.empty.then_some(true),
                errors,
            },
            extensions: (!self.setups.is_empty()).then(|| {
                BTreeMap::from([(
                    "dev.specgate.setups".to_string(),
                    serde_json::to_value(&self.setups).expect("normalized setups serialize"),
                )])
            }),
        })
    }
}

#[derive(Deserialize)]
struct NormalizedError {
    name: String,
    ty: String,
}

#[derive(Deserialize, Serialize)]
struct NormalizedSetup {
    fills: String,
    inputs: Vec<NormalizedInput>,
    output: String,
}

#[derive(Deserialize, Serialize)]
struct NormalizedInput {
    name: String,
    ty: String,
}

#[derive(Deserialize)]
struct NormalizedType {
    name: String,
    kind: String,
    #[serde(default)]
    fields: Vec<NormalizedField>,
    #[serde(default)]
    variants: Vec<NormalizedVariant>,
}

impl NormalizedType {
    fn to_ctsc(&self, context: &TypeContext<'_>) -> Result<RegistryNamedType, String> {
        let shape = match self.kind.as_str() {
            "struct" => {
                if !self.variants.is_empty() {
                    return Err(format!("struct type '{}' must not declare variants", self.name));
                }
                RegistryNamedTypeShape::Record {
                    fields: normalized_fields(&self.fields, context)?,
                }
            }
            "enum" => {
                if !self.fields.is_empty() {
                    return Err(format!("enum type '{}' must not declare fields", self.name));
                }
                if self.variants.is_empty() {
                    return Err(format!("enum type '{}' must declare at least one variant", self.name));
                }
                RegistryNamedTypeShape::TaggedUnion {
                    variants: self
                        .variants
                        .iter()
                        .map(|variant| {
                            if variant.tuple.is_some() && !variant.fields.is_empty() {
                                return Err(format!(
                                    "enum type '{}' variant '{}' must not declare both named and tuple payloads",
                                    self.name, variant.name
                                ));
                            }
                            let payload = if let Some(tuple) = &variant.tuple {
                                Some(RegistryTypeRef::Tuple {
                                    items: tuple
                                        .iter()
                                        .map(|field_type| schema_type_ref(field_type, context))
                                        .collect::<Result<Vec<_>, _>>()?,
                                })
                            } else if variant.fields.is_empty() {
                                None
                            } else {
                                Some(RegistryTypeRef::Record {
                                    fields: normalized_fields(&variant.fields, context)?,
                                })
                            };
                            Ok(RegistryVariant {
                                name: variant.name.clone(),
                                payload,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                }
            }
            other => return Err(format!("unsupported normalized type kind '{other}' for '{}'", self.name)),
        };

        Ok(RegistryNamedType {
            name: self.name.clone(),
            shape,
        })
    }
}

#[derive(Deserialize)]
struct NormalizedField {
    name: String,
    ty: String,
}

#[derive(Deserialize)]
struct NormalizedVariant {
    name: String,
    #[serde(default)]
    fields: Vec<NormalizedField>,
    tuple: Option<Vec<String>>,
}

fn normalized_fields(fields: &[NormalizedField], context: &TypeContext<'_>) -> Result<Vec<NamedValue>, String> {
    fields
        .iter()
        .map(|field| {
            Ok(NamedValue {
                name: field.name.clone(),
                value_type: schema_type_ref(&field.ty, context)?,
            })
        })
        .collect()
}

fn schema_type_ref(type_ref: &str, context: &TypeContext<'_>) -> Result<RegistryTypeRef, String> {
    let mut parser = TypeRefParser {
        input: type_ref,
        position: 0,
        context,
    };
    let parsed = parser.parse_type()?;
    parser.skip_whitespace();
    if parser.position != parser.input.len() {
        return Err(format!(
            "unexpected trailing input '{}' in type reference '{type_ref}'",
            &parser.input[parser.position..]
        ));
    }
    Ok(parsed)
}

struct TypeRefParser<'a> {
    input: &'a str,
    position: usize,
    context: &'a TypeContext<'a>,
}

impl TypeRefParser<'_> {
    fn parse_type(&mut self) -> Result<RegistryTypeRef, String> {
        self.skip_whitespace();
        if self.remaining().starts_with("()") {
            self.position += 2;
            return Ok(RegistryTypeRef::Primitive { name: "unit".to_string() });
        }

        let name = self.parse_identifier()?;
        self.skip_whitespace();
        if self.consume('<') {
            let arguments = self.parse_arguments()?;
            return Self::construct_generic(&name, arguments);
        }

        if is_ctsc_primitive(&name) {
            Ok(RegistryTypeRef::Primitive { name })
        } else {
            self.named_type(name)
        }
    }

    fn named_type(&self, qualified_name: String) -> Result<RegistryTypeRef, String> {
        if let Some((component, name)) = qualified_name.rsplit_once("::") {
            if component != self.context.component && !self.context.dependencies.contains(component) {
                return Err(format!(
                    "named type '{qualified_name}' references undeclared component dependency '{component}'"
                ));
            }
            let known = self
                .context
                .types_by_component
                .get(component)
                .is_some_and(|types| types.contains(name));
            if !known {
                return Err(format!("unknown named type '{qualified_name}'"));
            }
            return Ok(RegistryTypeRef::Named {
                name: name.to_string(),
                component_id: (component != self.context.component).then(|| component.to_string()),
                registry_id: None,
            });
        }
        let known = self
            .context
            .types_by_component
            .get(self.context.component)
            .is_some_and(|types| types.contains(&qualified_name));
        if known {
            Ok(RegistryTypeRef::Named {
                name: qualified_name,
                component_id: None,
                registry_id: None,
            })
        } else {
            Err(format!("unknown named type '{qualified_name}'"))
        }
    }

    fn parse_identifier(&mut self) -> Result<String, String> {
        self.skip_whitespace();
        let start = self.position;
        while let Some(character) = self.remaining().chars().next() {
            if character.is_alphanumeric() || matches!(character, '_' | ':' | '.') {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
        if self.position == start {
            Err(format!("expected type name at byte {}", self.position))
        } else {
            Ok(self.input[start..self.position].to_string())
        }
    }

    fn parse_arguments(&mut self) -> Result<Vec<RegistryTypeRef>, String> {
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.consume('>') {
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_type()?);
            self.skip_whitespace();
            if self.consume('>') {
                return Ok(arguments);
            }
            if self.consume(',') {
                continue;
            }
            if self.position == self.input.len() {
                return Err(format!("expected '>' at byte {}", self.position));
            }
            return Err(format!("expected ',' or '>' at byte {}", self.position));
        }
    }

    fn construct_generic(name: &str, mut arguments: Vec<RegistryTypeRef>) -> Result<RegistryTypeRef, String> {
        match name {
            "List" | "list" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::List {
                    items: Box::new(arguments.remove(0)),
                })
            }
            "Set" | "set" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::Set {
                    items: Box::new(arguments.remove(0)),
                })
            }
            "Map" | "map" => {
                expect_type_argument_count(name, &arguments, 2)?;
                let values = arguments.remove(1);
                let keys = arguments.remove(0);
                Ok(RegistryTypeRef::Map {
                    keys: Box::new(keys),
                    values: Box::new(values),
                })
            }
            "Tuple" | "tuple" => {
                if arguments.is_empty() {
                    return Err(format!("type constructor '{name}' expects at least 1 type argument"));
                }
                Ok(RegistryTypeRef::Tuple { items: arguments })
            }
            "Option" | "optional" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::Optional {
                    value: Box::new(arguments.remove(0)),
                })
            }
            other => Err(format!("unsupported type constructor '{other}'")),
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.remaining().starts_with(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.remaining().chars().next() {
            if character.is_whitespace() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
    }

    fn remaining(&self) -> &str {
        &self.input[self.position..]
    }
}

fn expect_type_argument_count(name: &str, arguments: &[RegistryTypeRef], expected: usize) -> Result<(), String> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(format!("type constructor '{name}' expects {expected} type arguments"))
    }
}

fn is_ctsc_primitive(name: &str) -> bool {
    matches!(
        name,
        "unit" | "string" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" | "bytes"
    )
}

fn is_schema_unit_type(type_ref: &str) -> bool {
    matches!(type_ref.trim(), "" | "()" | "unit")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryDocument {
    format: &'static str,
    format_version: &'static str,
    registry_id: String,
    version: String,
    components: Vec<RegistryComponent>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryDocumentWire {
    components: Vec<RegistryComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryComponent {
    id: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    dependencies: Vec<RegistryComponentRef>,
    operations: Vec<RegistryOperation>,
    types: Vec<RegistryNamedType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RegistryComponentRef {
    component_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryOperation {
    name: String,
    inputs: Vec<NamedValue>,
    observations: Vec<NamedValue>,
    outcomes: RegistryOutcomes,
    #[serde(skip_serializing_if = "Option::is_none")]
    extensions: Option<BTreeMap<String, serde_json::Value>>,
}

type NamedValue = ReplayRegistryInput;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryOutcomes {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<RegistryTypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    empty: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    errors: Vec<RegistryErrorOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryErrorOutcome {
    name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    value_type: Option<RegistryTypeRef>,
}

type RegistryTypeRef = ReplayType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryVariant {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<RegistryTypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryNamedType {
    name: String,
    #[serde(flatten)]
    shape: RegistryNamedTypeShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RegistryNamedTypeShape {
    Record { fields: Vec<NamedValue> },
    TaggedUnion { variants: Vec<RegistryVariant> },
}

fn value_to_any_value(value: &Value) -> AnyValue {
    match value {
        Value::String(value) => AnyValue::String(value.clone()),
        Value::Integer(value) => AnyValue::Int(value.to_string()),
        Value::Unsigned(value) => AnyValue::String(value.to_string()),
        Value::Float(value) if value.is_nan() => AnyValue::Double(DoubleValue::Symbol("NaN")),
        Value::Float(value) if value.is_infinite() && value.is_sign_positive() => AnyValue::Double(DoubleValue::Symbol("Infinity")),
        Value::Float(value) if value.is_infinite() => AnyValue::Double(DoubleValue::Symbol("-Infinity")),
        Value::Float(value) => AnyValue::Double(DoubleValue::Number(*value)),
        Value::Bool(value) => AnyValue::Bool(*value),
        Value::List(values) => AnyValue::Array(ArrayValue {
            values: values.iter().map(value_to_any_value).collect(),
        }),
        Value::Map(values) => AnyValue::Kvlist(KeyValueList {
            values: values
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone(),
                    value: value_to_any_value(value),
                })
                .collect(),
        }),
        Value::Set(values) => AnyValue::Array(ArrayValue {
            values: values.iter().map(value_to_any_value).collect(),
        }),
    }
}

fn string_attribute(key: &'static str, value: impl Into<String>) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: AnyValue::String(value.into()),
    }
}

fn integer_attribute(key: &'static str, value: i64) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: AnyValue::Int(value.to_string()),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OtlpDocument {
    resource_spans: Vec<ResourceSpans>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpans {
    resource: Resource,
    scope_spans: Vec<ScopeSpans>,
    schema_url: String,
}

#[derive(Serialize)]
struct Resource {
    attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSpans {
    scope: InstrumentationScope,
    spans: Vec<Span>,
    schema_url: String,
}

#[derive(Serialize)]
struct InstrumentationScope {
    name: &'static str,
    version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Span {
    trace_id: String,
    name: &'static str,
    #[serde(rename = "spanId")]
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentSpanId")]
    parent_id: Option<String>,
    kind: i32,
    start_time_unix_nano: String,
    end_time_unix_nano: String,
    attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    events: Vec<SpanEvent>,
    status: Status,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpanEvent {
    time_unix_nano: String,
    name: &'static str,
    attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
struct Status {
    code: i32,
}

#[derive(Serialize)]
struct KeyValue {
    key: String,
    value: AnyValue,
}

#[derive(Serialize)]
enum AnyValue {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(DoubleValue),
    #[serde(rename = "arrayValue")]
    Array(ArrayValue),
    #[serde(rename = "kvlistValue")]
    Kvlist(KeyValueList),
}

#[derive(Serialize)]
#[serde(untagged)]
enum DoubleValue {
    Number(f64),
    Symbol(&'static str),
}

#[derive(Serialize)]
struct ArrayValue {
    values: Vec<AnyValue>,
}

#[derive(Serialize)]
struct KeyValueList {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    values: Vec<KeyValue>,
}

#[cfg(test)]
mod tests {
    //! Unit coverage for the private registry-document ordering and merge
    //! rules. The fixture corpus cannot distinguish alphabetical ordering from
    //! a selected-component-first rule, because every multi-component fixture
    //! registry happens to name a selected component that already sorts first.
    //! These synthetic schemas pin the rule instead.

    use super::*;

    /// One component whose only dependency sorts strictly before it.
    fn dependency_sorts_first_schema() -> String {
        serde_json::json!({
            "component": "zeta.app",
            "dependencies": ["alpha.core"],
            "dependency_types": [{
                "component": "alpha.core",
                "types": [{"name": "Widget", "kind": "struct", "fields": [{"name": "id", "ty": "i32"}]}]
            }],
            "operations": [{
                "name": "run",
                "is_async": false,
                "inputs": [{"name": "value", "ty": "i32"}],
                "output": "alpha.core::Widget"
            }],
            "types": []
        })
        .to_string()
    }

    fn component_ids(registry_json: &str) -> Vec<String> {
        let document: serde_json::Value = serde_json::from_str(registry_json).expect("registry JSON");
        document["components"]
            .as_array()
            .expect("components array")
            .iter()
            .map(|component| component["id"].as_str().expect("component id").to_string())
            .collect()
    }

    fn component(json: serde_json::Value) -> RegistryComponent {
        serde_json::from_value(json).expect("registry component")
    }

    fn operation(name: &str, output: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "inputs": [],
            "observations": [],
            "outcomes": {"result": {"kind": "primitive", "name": output}}
        })
    }

    fn named_type(name: &str, field: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "kind": "record",
            "fields": [{"name": field, "type": {"kind": "primitive", "name": "i32"}}]
        })
    }

    /// Components are ordered by id alone. The selected component gets no
    /// privileged first position, so a dependency that sorts before it leads.
    #[test]
    fn registry_components_are_ordered_alphabetically_by_id() {
        let encoded = encode_schema_registry_result(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            &dependency_sorts_first_schema(),
        )
        .expect("schema encodes");
        assert_eq!(component_ids(&encoded.registry_json), vec!["alpha.core", "zeta.app"]);
        assert_eq!(encoded.operation_count, 1);
        assert_eq!(encoded.type_count, 1);
    }

    /// `discover` encodes one schema and `capture` encodes a set, but both
    /// write the same `registry.ctsc.json`. One ordering rule governs both, so
    /// the same logical component set must serialize to the same bytes.
    #[test]
    fn single_and_multi_schema_encoders_agree_byte_for_byte() {
        let schema = dependency_sorts_first_schema();
        let single =
            encode_schema_registry_result("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &schema).expect("single encode");
        let many = encode_schema_registries_result(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            std::slice::from_ref(&schema),
        )
        .expect("multi encode");
        assert_eq!(single.registry_json.as_bytes(), many.registry_json.as_bytes());
        assert_eq!(single.operation_count, many.operation_count);
        assert_eq!(single.type_count, many.type_count);

        // Repeating a schema merges it into itself and must stay identical.
        let repeated = encode_schema_registries_result(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            &[schema.clone(), schema],
        )
        .expect("repeated encode");
        assert_eq!(repeated.registry_json.as_bytes(), single.registry_json.as_bytes());
        assert_eq!(repeated.operation_count, single.operation_count);
        assert_eq!(repeated.type_count, single.type_count);
    }

    /// The one shape where the merged document counts exceed every individual
    /// schema's: `alpha.core` contributes its own operations from its full
    /// schema *and* appears again as `zeta.app`'s dependency type closure.
    /// Merging must union the two views rather than let either replace the
    /// other, so the counts cover both components' whole surface.
    #[test]
    fn merged_counts_cover_a_component_present_as_both_schema_and_dependency() {
        let core = serde_json::json!({
            "component": "alpha.core",
            "operations": [{
                "name": "build",
                "is_async": false,
                "inputs": [{"name": "id", "ty": "i32"}],
                "output": "alpha.core::Widget"
            }],
            "types": [{"name": "Widget", "kind": "struct", "fields": [{"name": "id", "ty": "i32"}]}]
        })
        .to_string();
        let app = dependency_sorts_first_schema();

        let merged = encode_schema_registries_result(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            &[app.clone(), core.clone()],
        )
        .expect("merged encode");

        // `zeta.app::run` plus `alpha.core::build`; the dependency closure view
        // of `alpha.core` carries no operations and must not erase them.
        assert_eq!(merged.operation_count, 2);
        // `alpha.core::Widget` counted exactly once despite both views
        // declaring it.
        assert_eq!(merged.type_count, 1);
        assert_eq!(component_ids(&merged.registry_json), vec!["alpha.core", "zeta.app"]);

        // Both counts strictly exceed what either schema yields alone, which is
        // what the single-schema byte-equivalence test cannot reach.
        let app_only =
            encode_schema_registry_result("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &app).expect("app encode");
        assert_eq!((app_only.operation_count, app_only.type_count), (1, 1));
        let core_only =
            encode_schema_registry_result("urn:ctsc:registry:alpha.core".to_string(), "0.1.0".to_string(), &core).expect("core encode");
        assert_eq!((core_only.operation_count, core_only.type_count), (1, 1));
        assert!(merged.operation_count > app_only.operation_count && merged.operation_count > core_only.operation_count);

        // Schema order must not change the document.
        let reversed = encode_schema_registries_result("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &[core, app])
            .expect("reversed encode");
        assert_eq!(reversed.registry_json.as_bytes(), merged.registry_json.as_bytes());
    }

    /// Repeated declarations of one component contribute the union of their
    /// dependencies, operations, and types, each kept in sorted order.
    #[test]
    fn merge_registry_component_unions_declarations_deterministically() {
        let mut existing = component(serde_json::json!({
            "id": "zeta.app",
            "dependencies": [{"componentId": "beta.core"}],
            "operations": [operation("run", "i32")],
            "types": [named_type("Widget", "id")]
        }));
        let incoming = component(serde_json::json!({
            "id": "zeta.app",
            "dependencies": [{"componentId": "alpha.core"}, {"componentId": "beta.core"}],
            "operations": [operation("advance", "i32"), operation("run", "i32")],
            "types": [named_type("Gadget", "id"), named_type("Widget", "id")]
        }));
        merge_registry_component(&mut existing, incoming).expect("compatible declarations merge");

        assert_eq!(
            existing
                .dependencies
                .iter()
                .map(|dependency| dependency.component_id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha.core", "beta.core"],
            "dependencies are unioned and sorted"
        );
        assert_eq!(
            existing.operations.iter().map(|op| op.name.as_str()).collect::<Vec<_>>(),
            vec!["advance", "run"]
        );
        assert_eq!(
            existing.types.iter().map(|ty| ty.name.as_str()).collect::<Vec<_>>(),
            vec!["Gadget", "Widget"]
        );

        // Merging the same content again is a fixpoint: no duplicates, no
        // reordering, so declaration order cannot leak into the document.
        let again = existing.clone();
        let mut fixpoint = existing.clone();
        merge_registry_component(&mut fixpoint, again).expect("idempotent merge");
        assert_eq!(fixpoint, existing);
    }

    /// Two declarations of the same operation or type name that disagree are
    /// reported rather than silently resolved to one of them.
    #[test]
    fn merge_registry_component_rejects_disagreeing_declarations() {
        let base = component(serde_json::json!({
            "id": "zeta.app",
            "operations": [operation("run", "i32")],
            "types": [named_type("Widget", "id")]
        }));

        let mut operations = base.clone();
        let error = merge_registry_component(
            &mut operations,
            component(serde_json::json!({
                "id": "zeta.app",
                "operations": [operation("run", "i64")],
                "types": []
            })),
        )
        .expect_err("a divergent operation must be reported");
        assert_eq!(error, "normalized schemas disagree on operation 'zeta.app::run'");

        let mut types = base;
        let error = merge_registry_component(
            &mut types,
            component(serde_json::json!({
                "id": "zeta.app",
                "operations": [],
                "types": [named_type("Widget", "label")]
            })),
        )
        .expect_err("a divergent type must be reported");
        assert_eq!(error, "normalized schemas disagree on type 'zeta.app::Widget'");
    }
}
