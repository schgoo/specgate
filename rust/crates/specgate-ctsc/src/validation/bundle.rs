use super::model::{AnyValue, RegistrySet, TraceDocument};
use super::{ValidationIssue, is_digest, issue, load_registry_set, load_trace, located, read_bytes, sha256_digest, validate_linked_model};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::path::Path;

const CAPTURE_FORMAT: &str = "specgate.capture-manifest";
const CAPTURE_VERSION: &str = "0.1.0";

pub(crate) fn validate(directory: &Path) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let manifest_path = directory.join("manifest.json");
    let registry_path = directory.join("registry.ctsc.json");
    let trace_path = directory.join("reference.otlp.json");
    let manifest_bytes = read_bytes(&manifest_path, &mut issues);
    let registry_bytes = read_bytes(&registry_path, &mut issues);
    let trace_bytes = read_bytes(&trace_path, &mut issues);
    let manifest = manifest_bytes
        .as_deref()
        .and_then(|bytes| parse_manifest(bytes, &manifest_path, &mut issues));
    let registry = load_registry_set(&registry_path, &[]);
    let trace = load_trace(&trace_path);
    issues.extend(registry.issues);
    issues.extend(trace.issues);

    if let (Some(manifest), Some(registry_bytes), Some(trace_bytes)) = (manifest.as_ref(), registry_bytes.as_ref(), trace_bytes.as_ref()) {
        validate_digests(manifest, registry_bytes, trace_bytes, &manifest_path, &mut issues);
    }
    if issues.is_empty()
        && let (Some(manifest), Some(registry), Some(trace)) = (manifest.as_ref(), registry.value.as_ref(), trace.value.as_ref())
    {
        validate_linked_model(trace, registry, &mut issues);
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
    path: String,
    id: String,
    version: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestReference {
    path: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestScenarios {
    count: i32,
    names: Vec<String>,
}

fn parse_manifest(bytes: &[u8], path: &Path, issues: &mut Vec<ValidationIssue>) -> Option<CaptureManifest> {
    let manifest = match serde_json::from_slice::<CaptureManifest>(bytes) {
        Ok(manifest) => manifest,
        Err(error) => {
            issue(issues, located(path, "$"), format!("invalid capture manifest JSON: {error}"));
            return None;
        }
    };
    require(
        manifest.format == CAPTURE_FORMAT,
        path,
        "$.format",
        "format must be 'specgate.capture-manifest'",
        issues,
    );
    require(
        manifest.format_version == CAPTURE_VERSION,
        path,
        "$.formatVersion",
        "formatVersion must be '0.1.0'",
        issues,
    );
    require(
        manifest.registry.path == "registry.ctsc.json",
        path,
        "$.registry.path",
        "registry path must be 'registry.ctsc.json'",
        issues,
    );
    require(
        manifest.reference.path == "reference.otlp.json",
        path,
        "$.reference.path",
        "reference path must be 'reference.otlp.json'",
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

fn validate_digests(manifest: &CaptureManifest, registry_bytes: &[u8], trace_bytes: &[u8], path: &Path, issues: &mut Vec<ValidationIssue>) {
    let registry_digest = sha256_digest(registry_bytes);
    require(
        manifest.registry.digest == registry_digest,
        path,
        "$.registry.digest",
        &format!(
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
        &format!(
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
    path: &Path,
    issues: &mut Vec<ValidationIssue>,
) {
    require(
        manifest.registry.id == registry.root_id,
        path,
        "$.registry.id",
        "manifest registry ID does not match registry document",
        issues,
    );
    require(
        manifest.registry.version == registry.root_version,
        path,
        "$.registry.version",
        "manifest registry version does not match registry document",
        issues,
    );
    require(
        registry.components.contains_key(&manifest.component_id),
        path,
        "$.componentId",
        "manifest component is absent from registry",
        issues,
    );
    let runs = trace.spans.iter().filter(|span| span.name == "conformance.run").collect::<Vec<_>>();
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
                .get("conformance.target.name")
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.target.name
        ) {
            issue(issues, span.location.clone(), "trace target name does not match capture manifest");
        }
        if !matches!(
            span.resource_attributes
                .get("conformance.target.language")
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.target.language
        ) {
            issue(
                issues,
                span.location.clone(),
                "trace target language does not match capture manifest",
            );
        }
        if !matches!(
            span.resource_attributes
                .get("conformance.tool.name")
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.tool.name
        ) {
            issue(issues, span.location.clone(), "trace tool name does not match capture manifest");
        }
        if !matches!(
            span.resource_attributes
                .get("conformance.tool.version")
                .and_then(AnyValue::as_string),
            Some(value) if value == manifest.tool.version
        ) {
            issue(issues, span.location.clone(), "trace tool version does not match capture manifest");
        }
    }
    let mut scenarios = trace
        .spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span.name == "conformance.scenario" && span.trace_id == run.trace_id && span.parent_span_id == run.span_id)
        .map(|(position, span)| {
            (
                span.attributes
                    .get("conformance.scenario.index")
                    .and_then(AnyValue::as_int)
                    .unwrap_or(i64::MAX),
                position,
                span.attributes
                    .get("conformance.scenario.name")
                    .and_then(AnyValue::as_string)
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect::<Vec<_>>();
    scenarios.sort_by_key(|(index, position, _)| (*index, *position));
    for (expected, (actual, _, _)) in scenarios.iter().enumerate() {
        require(
            i64::try_from(expected).ok() == Some(*actual),
            path,
            "$.scenarios",
            &format!("scenario indexes must be unique and contiguous from zero; expected {expected}, found {actual}"),
            issues,
        );
    }
    let names = scenarios.into_iter().map(|(_, _, name)| name).collect::<Vec<_>>();
    require(
        names == manifest.scenarios.names,
        path,
        "$.scenarios.names",
        &format!(
            "trace scenario order/names {names:?} do not match capture manifest {:?}",
            manifest.scenarios.names
        ),
        issues,
    );
}

fn require(condition: bool, path: &Path, location: &str, message: &str, issues: &mut Vec<ValidationIssue>) {
    if !condition {
        issue(issues, located(path, location), message);
    }
}
