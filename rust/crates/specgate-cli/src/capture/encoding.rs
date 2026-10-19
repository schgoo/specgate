//! Deterministic CTSC capture bundle encoding.
use super::facade::BundleRequest;
use super::filter::filter;
use super::{
    BTreeMap, BTreeSet, CaptureManifest, CaptureReport, ComponentId, Discovered, ExecutedTests, FORMAT_VERSION, FailureContext, Language,
    MANIFEST_FILE, MANIFEST_FORMAT, Path, PathBuf, REGISTRY_FILE, REGISTRY_PREFIX, REGISTRY_VERSION, Reference, RegistryDto, RegistryId,
    Scenarios, TRACE_FILE, TargetDto, Tool, Version, WireDigest, artifact_path, ctsc_capture, encode_otlp, encode_registries,
    normalize_registry, sha256_digest,
};
use specgate_ctsc::validation::DocumentBytes;

/// Stable capture-manifest producer identity; changing it breaks consumers that identify `SpecGate` artifacts.
const TOOL_NAME: &str = "specgate";

pub(super) struct EncodedBundle {
    pub(super) out: PathBuf,
    pub(super) registry_bytes: Vec<u8>,
    pub(super) trace_bytes: Vec<u8>,
    pub(super) manifest_bytes: Vec<u8>,
    pub(super) report: CaptureReport,
}

/// Normalized schemas, as serialized JSON, of the components nested inside a
/// capture, keyed by component id.
pub(super) type NestedSchemas = BTreeMap<String, String>;

/// Refuse to publish a bundle whose trace does not link to its own registry.
fn reject_unlinked(trace: &[u8], registry: &[u8], component: &str) -> Result<(), FailureContext> {
    let report = specgate_ctsc::validation::bytes::validate_linked(
        DocumentBytes::new(Path::new(TRACE_FILE.0), trace),
        DocumentBytes::new(Path::new(REGISTRY_FILE.0), registry),
        [],
    );
    if report.valid {
        return Ok(());
    }
    Err(FailureContext::domain(format!(
        "captured trace for component '{component}' does not link to its registry: {}",
        report
            .issues
            .iter()
            .map(|issue| format!("{}: {}", issue.location, issue.message))
            .collect::<Vec<_>>()
            .join("; ")
    )))
}

pub(super) fn encode_bundle(
    discovered: &Discovered,
    request: &BundleRequest,
    executed: &ExecutedTests,
    nested: &NestedSchemas,
) -> Result<EncodedBundle, FailureContext> {
    let resolved = &discovered.implementation;
    let language = resolved.language();
    // The artifact label is the bundle directory's name, so label and path always agree.
    let target_name = request
        .out
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(specgate_discovery::identity::ImplementationLabel::from)
        .ok_or_else(|| FailureContext::domain("capture output directory must end in a named UTF-8 directory"))?;
    let selected_id = &request.component;
    let selected = selected_id.as_str();
    let captures = filter(&executed.captures, selected_id);
    if captures.is_empty() {
        let failures = if executed.failures.is_empty() {
            String::new()
        } else {
            format!(
                "; these tests produced no capture and were skipped:\n  {}",
                executed
                    .failures
                    .iter()
                    .map(|failure| format!("{}: {}", failure.scenario_name, failure.summary))
                    .collect::<Vec<_>>()
                    .join("\n  ")
            )
        };
        return Err(FailureContext::domain(format!(
            "no passing tests captured operations for component '{selected}'; add a handwritten test that invokes the component{failures}"
        )));
    }

    // Component capture keeps nested calls into other annotated components
    // verbatim, so the exported registry must declare every component the
    // filtered trace actually contains or CTSC Linked validation rejects those
    // nested spans. The selected component always contributes its full
    // normalized surface, so a single-component capture is unchanged.
    let mut components = BTreeSet::from([selected]);
    for capture in &captures {
        for operation in &capture.operations {
            components.insert(operation.component_id.as_str());
        }
    }
    let mut schemas = Vec::with_capacity(components.len());
    for component in &components {
        // A language whose discovery reports one component at a time supplies the
        // nested components' normalized schemas explicitly; otherwise the selected
        // discovery registry describes every component.
        let schema = if let Some(schema) = nested.get(*component).filter(|_| *component != selected) {
            schema.clone()
        } else {
            let schema = normalize_registry(&discovered.registry, language, *component)?;
            serde_json::to_string(&schema)
                .map_err(|error| FailureContext::with_source(format!("failed to serialize normalized discovery schema: {error}"), error))?
        };
        schemas.push(schema);
    }
    let registry_id = format!("{REGISTRY_PREFIX}{selected}");
    let schemas = schemas.into_iter().map(specgate_ctsc::registry::Schema::new).collect::<Vec<_>>();
    let registry_encoding = encode_registries(registry_id.clone(), REGISTRY_VERSION.to_string(), &schemas)
        .map_err(|error| FailureContext::with_source(format!("failed to encode discovered registry: {error}"), error))?;
    let registry_bytes = registry_encoding.registry_json.into_bytes();
    let registry_digest = sha256_digest(&registry_bytes);

    let operation_count = captures.iter().try_fold(0_usize, |count, capture| {
        count
            .checked_add(capture.operations.len())
            .ok_or_else(|| "captured operation count overflow".to_string())
    })?;
    let metadata = ctsc_capture::Metadata::new(
        env!("CARGO_PKG_VERSION"),
        ctsc_capture::Target::new(target_name.as_str(), language.as_str()),
        ctsc_capture::Registry::new(&registry_id, REGISTRY_VERSION, &registry_digest),
    );
    let trace_encoding = encode_otlp(&captures, &metadata)?;
    let trace_bytes = trace_encoding.otlp_json.into_bytes();
    reject_unlinked(&trace_bytes, &registry_bytes, selected)?;
    let trace_digest = sha256_digest(&trace_bytes);
    let scenario_count = captures.len();
    let report_scenarios = u32::try_from(scenario_count).map_err(|_error| "captured scenario count exceeds u32".to_string())?;
    let operations = u32::try_from(operation_count).map_err(|_error| "captured operation count exceeds u32".to_string())?;
    let manifest = CaptureManifest {
        format: MANIFEST_FORMAT,
        format_version: Version::try_from(FORMAT_VERSION).expect("FORMAT_VERSION must be a non-empty manifest version"),
        component_id: selected_id.clone(),
        target: TargetDto {
            name: target_name,
            language: Language::from(language),
        },
        tool: Tool {
            name: TOOL_NAME,
            version: Version::try_from(env!("CARGO_PKG_VERSION"))?,
        },
        registry: RegistryDto {
            path: REGISTRY_FILE,
            id: RegistryId::try_from(registry_id)?,
            version: Version::try_from(REGISTRY_VERSION)?,
            digest: WireDigest::try_from(registry_digest)?,
        },
        reference: Reference {
            path: TRACE_FILE,
            digest: WireDigest::try_from(trace_digest)?,
        },
        scenarios: Scenarios {
            count: scenario_count,
            names: captures.iter().map(|capture| capture.scenario_name.clone()).collect(),
        },
    };
    let manifest_bytes = serde_json::to_vec(&manifest)
        .map_err(|error| FailureContext::with_source(format!("failed to serialize capture manifest: {error}"), error))?;
    Ok(EncodedBundle {
        out: request.out.clone(),
        registry_bytes,
        trace_bytes,
        manifest_bytes,
        report: CaptureReport {
            component_id: ComponentId::new(selected),
            scenarios: report_scenarios,
            operations,
            registry_path: artifact_path(&request.out, REGISTRY_FILE),
            trace_path: artifact_path(&request.out, TRACE_FILE),
            manifest_path: artifact_path(&request.out, MANIFEST_FILE),
        },
    })
}
