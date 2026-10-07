use super::model::{AnyValue, RegistrySet, TraceDocument};
use super::{
    BundleBytes, DocumentBytes, Loaded, ValidationIssue, check_linked, is_digest, issue, load_bytes, located, registry, sha256_digest,
};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Capture-manifest media type defined by `SpecGate`'s bundle contract.
/// Changing it requires a coordinated reader/writer compatibility revision.
const CAPTURE_FORMAT: &str = "specgate.capture-manifest";
/// Current capture-manifest schema version from the bundle contract.
/// It is independent of the CTSC registry/trace version.
const CAPTURE_VERSION: &str = "0.1.0";
/// Missing scenario indexes sort after every valid index for deterministic diagnostics.
const MISSING_INDEX: i64 = i64::MAX;
// Filenames fixed by the CTSC capture-bundle contract; changing them breaks artifact compatibility.
const MANIFEST_FILE: &str = "manifest.json";
const REGISTRY_FILE: &str = "registry.ctsc.json";
const REFERENCE_FILE: &str = "reference.otlp.json";
// CTSC Trace Core sentinel and resource keys; changing these requires a coordinated contract version change.
const RUN_NAME: &str = "conformance.run";
const SCENARIO_NAME: &str = "conformance.scenario";
const TARGET_NAME: &str = "conformance.target.name";
const TARGET_LANGUAGE: &str = "conformance.target.language";
const TOOL_NAME: &str = "conformance.tool.name";
const TOOL_VERSION: &str = "conformance.tool.version";
const SCENARIO_INDEX: &str = "conformance.scenario.index";
const SCENARIO_ATTR: &str = "conformance.scenario.name";

pub(crate) fn validate(directory: impl AsRef<Path>) -> Vec<ValidationIssue> {
    validate_read(directory, &crate::comparison::SystemReader::system())
}

pub(crate) fn validate_read(directory: impl AsRef<Path>, reader: &impl crate::comparison::DocumentReader) -> Vec<ValidationIssue> {
    let directory = directory.as_ref();
    let mut issues = Vec::new();
    let manifest_path = directory.join(MANIFEST_FILE);
    let registry_path = directory.join(REGISTRY_FILE);
    let trace_path = directory.join(REFERENCE_FILE);
    let manifest_bytes = super::finish_read(&manifest_path, reader.read(&manifest_path), &mut issues);
    let registry_bytes = super::finish_read(&registry_path, reader.read(&registry_path), &mut issues);
    let trace_bytes = super::finish_read(&trace_path, reader.read(&trace_path), &mut issues);
    let registry = registry_bytes.as_deref().map_or_else(
        || registry::load_reader(&registry_path, &[] as &[&Path], reader),
        |bytes| registry::load_bytes(DocumentBytes::new(&registry_path, bytes), &[]),
    );
    let trace = trace_bytes.as_deref().map_or_else(
        || super::trace::load_from(&trace_path, reader),
        |bytes| load_bytes(&trace_path, bytes),
    );
    validate_loaded(
        directory,
        manifest_bytes.as_deref(),
        registry_bytes.as_deref(),
        trace_bytes.as_deref(),
        registry,
        trace,
        issues,
    )
}

pub(crate) fn validate_bytes(directory: impl AsRef<Path>, documents: BundleBytes<'_>) -> Vec<ValidationIssue> {
    let directory = directory.as_ref();
    let manifest_bytes = documents.manifest.bytes;
    let registry_bytes = documents.registry.bytes;
    let trace_bytes = documents.trace.bytes;
    let registry_path = directory.join(REGISTRY_FILE);
    let trace_path = directory.join(REFERENCE_FILE);
    let registry = registry::load_bytes(DocumentBytes::new(&registry_path, registry_bytes), &[]);
    let trace = load_bytes(&trace_path, trace_bytes);
    validate_loaded(
        directory,
        Some(manifest_bytes),
        Some(registry_bytes),
        Some(trace_bytes),
        registry,
        trace,
        Vec::new(),
    )
}

fn validate_loaded(
    directory: impl AsRef<Path>,
    manifest_bytes: Option<&[u8]>,
    registry_bytes: Option<&[u8]>,
    trace_bytes: Option<&[u8]>,
    registry: Loaded<RegistrySet>,
    trace: Loaded<TraceDocument>,
    mut issues: Vec<ValidationIssue>,
) -> Vec<ValidationIssue> {
    let directory = directory.as_ref();
    let manifest_path = directory.join(MANIFEST_FILE);
    let manifest = manifest_bytes.and_then(|bytes| parse_manifest(bytes, &manifest_path, &mut issues));
    issues.extend(registry.issues);
    issues.extend(trace.issues);
    if let (Some(manifest), Some(registry_bytes), Some(trace_bytes)) = (manifest.as_ref(), registry_bytes.as_ref(), trace_bytes.as_ref()) {
        validate_digests(manifest, registry_bytes, trace_bytes, &manifest_path, &mut issues);
    }
    if issues.is_empty()
        && let (Some(manifest), Some(registry), Some(trace)) = (manifest.as_ref(), registry.value.as_ref(), trace.value.as_ref())
    {
        check_linked(trace, registry, &mut issues);
        validate_semantics(manifest, registry, trace, &manifest_path, &mut issues);
    }
    issues
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CaptureManifest {
    format: String,
    format_version: String,
    component_id: String,
    target: ManifestTarget,
    tool: ManifestTool,
    registry: ManifestRegistry,
    reference: ManifestReference,
    scenarios: ManifestScenarios,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTarget {
    name: String,
    language: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestTool {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestRegistry {
    path: PathBuf,
    id: String,
    version: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestReference {
    path: PathBuf,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestScenarios {
    count: i32,
    names: Vec<String>,
}

fn parse_manifest(bytes: impl AsRef<[u8]>, path: impl AsRef<Path>, issues: &mut Vec<ValidationIssue>) -> Option<CaptureManifest> {
    let bytes = bytes.as_ref();
    let path = path.as_ref();
    let manifest = match serde_json::from_slice::<CaptureManifest>(bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            issue(located(path, "$"), format!("invalid capture manifest JSON: {error}"), issues);
            return None;
        }
    };
    require(
        manifest.format == CAPTURE_FORMAT,
        path,
        "$.format",
        format!("format must be '{CAPTURE_FORMAT}'"),
        issues,
    );
    require(
        manifest.format_version == CAPTURE_VERSION,
        path,
        "$.formatVersion",
        format!("formatVersion must be '{CAPTURE_VERSION}'"),
        issues,
    );
    require(
        manifest.registry.path == Path::new(REGISTRY_FILE),
        path,
        "$.registry.path",
        format!("registry path must be '{REGISTRY_FILE}'"),
        issues,
    );
    require(
        manifest.reference.path == Path::new(REFERENCE_FILE),
        path,
        "$.reference.path",
        format!("reference path must be '{REFERENCE_FILE}'"),
        issues,
    );
    for (location, value) in [
        ("$.componentId", manifest.component_id.as_str()),
        ("$.target.name", manifest.target.name.as_str()),
        ("$.target.language", manifest.target.language.as_str()),
        ("$.tool.name", manifest.tool.name.as_str()),
        ("$.tool.version", manifest.tool.version.as_str()),
        ("$.registry.id", manifest.registry.id.as_str()),
        ("$.registry.version", manifest.registry.version.as_str()),
    ] {
        require(!value.is_empty(), path, location, "identity field must not be empty", issues);
    }
    require(
        is_digest(&manifest.registry.digest),
        path,
        "$.registry.digest",
        "digest must be 'sha256:' followed by 64 lowercase hexadecimal characters",
        issues,
    );
    require(
        is_digest(&manifest.reference.digest),
        path,
        "$.reference.digest",
        "digest must be 'sha256:' followed by 64 lowercase hexadecimal characters",
        issues,
    );
    require(
        manifest.scenarios.count > 0,
        path,
        "$.scenarios.count",
        "capture manifest must declare at least one scenario",
        issues,
    );
    require(
        usize::try_from(manifest.scenarios.count).ok() == Some(manifest.scenarios.names.len()),
        path,
        "$.scenarios",
        "scenario count does not match scenario names",
        issues,
    );
    require(
        manifest.scenarios.names.iter().collect::<BTreeSet<_>>().len() == manifest.scenarios.names.len(),
        path,
        "$.scenarios.names",
        "capture manifest contains duplicate scenario names",
        issues,
    );
    Some(manifest)
}

fn validate_digests(
    manifest: &CaptureManifest,
    registry_bytes: impl AsRef<[u8]>,
    trace_bytes: impl AsRef<[u8]>,
    path: impl AsRef<Path>,
    issues: &mut Vec<ValidationIssue>,
) {
    let registry_bytes = registry_bytes.as_ref();
    let trace_bytes = trace_bytes.as_ref();
    let path = path.as_ref();
    let registry_digest = sha256_digest(registry_bytes);
    require(
        manifest.registry.digest == registry_digest,
        path,
        "$.registry.digest",
        format!(
            "manifest declares '{}' but exact registry bytes digest to '{registry_digest}'",
            manifest.registry.digest
        ),
        issues,
    );
    let reference_digest = sha256_digest(trace_bytes);
    require(
        manifest.reference.digest == reference_digest,
        path,
        "$.reference.digest",
        format!(
            "manifest declares '{}' but exact reference bytes digest to '{reference_digest}'",
            manifest.reference.digest
        ),
        issues,
    );
}

fn validate_semantics(
    manifest: &CaptureManifest,
    registry: &RegistrySet,
    trace: &TraceDocument,
    path: impl AsRef<Path>,
    issues: &mut Vec<ValidationIssue>,
) {
    let path = path.as_ref();
    require(
        manifest.registry.id == registry.root_id.as_str(),
        path,
        "$.registry.id",
        "manifest registry ID does not match registry document",
        issues,
    );
    require(
        manifest.registry.version == registry.root_version.as_str(),
        path,
        "$.registry.version",
        "manifest registry version does not match registry document",
        issues,
    );
    require(
        registry.components.contains_key(manifest.component_id.as_str()),
        path,
        "$.componentId",
        "manifest component is absent from registry",
        issues,
    );
    let runs = trace.spans.iter().filter(|span| span.name == RUN_NAME).collect::<Vec<_>>();
    require(
        runs.len() == 1,
        path,
        "$.scenarios",
        "capture bundle trace must contain exactly one run",
        issues,
    );
    let Some(run) = runs.first() else {
        return;
    };
    for span in &trace.spans {
        if !matches!(
            span.resource_attributes
                .get(TARGET_NAME)
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.target.name
        ) {
            issue(span.location.clone(), "trace target name does not match capture manifest", issues);
        }
        if !matches!(
            span.resource_attributes
                .get(TARGET_LANGUAGE)
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.target.language
        ) {
            issue(
                span.location.clone(),
                "trace target language does not match capture manifest",
                issues,
            );
        }
        if !matches!(
            span.resource_attributes
                .get(TOOL_NAME)
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.tool.name
        ) {
            issue(span.location.clone(), "trace tool name does not match capture manifest", issues);
        }
        if !matches!(
            span.resource_attributes
                .get(TOOL_VERSION)
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.tool.version
        ) {
            issue(span.location.clone(), "trace tool version does not match capture manifest", issues);
        }
    }
    let mut scenarios = trace
        .spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span.name == SCENARIO_NAME && span.trace_id == run.trace_id && span.parent_span_id == run.span_id)
        .map(|(position, span)| {
            (
                span.attributes
                    .get(SCENARIO_INDEX)
                    .and_then(AnyValue::as_int)
                    .unwrap_or(MISSING_INDEX),
                position,
                span.attributes
                    .get(SCENARIO_ATTR)
                    .and_then(AnyValue::as_string)
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    scenarios.sort_by_key(|(index, position, _)| (*index, *position));
    for (expected, (actual, _, _)) in scenarios.iter().enumerate() {
        if i64::try_from(expected).ok() != Some(*actual) {
            issue(
                located(path, "$.scenarios"),
                format!("scenario indexes must be unique and contiguous from zero; expected {expected}, found {actual}"),
                issues,
            );
        }
    }
    let names = scenarios.into_iter().map(|(_, _, name)| name).collect::<Vec<_>>();
    require(
        names == manifest.scenarios.names,
        path,
        "$.scenarios.names",
        format!(
            "trace scenario order/names {names:?} do not match capture manifest {:?}",
            manifest.scenarios.names
        ),
        issues,
    );
}

fn require(
    condition: bool,
    path: impl AsRef<Path>,
    location: impl AsRef<str>,
    message: impl AsRef<str>,
    issues: &mut Vec<ValidationIssue>,
) {
    if !condition {
        issue(located(path.as_ref(), location.as_ref()), message.as_ref(), issues);
    }
}
