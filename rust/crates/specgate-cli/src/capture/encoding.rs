//! Deterministic CTSC capture bundle encoding.
use super::facade::BundleRequest;
use super::filter::filter;
use super::{
    BTreeSet, CaptureManifest, CaptureReport, ComponentId, ContextError, ExecutedTests, FORMAT_VERSION, Language, MANIFEST_FILE,
    MANIFEST_FORMAT, PathBuf, REGISTRY_FILE, REGISTRY_PREFIX, REGISTRY_VERSION, Reference, RegistryDto, RegistryId, Scenarios, TRACE_FILE,
    Target, TargetDto, Tool, Version, WireDigest, artifact_path, ctsc_capture, encode_otlp, encode_registries, normalize_registry,
    sha256_digest,
};

/// Stable capture-manifest producer identity; changing it breaks consumers that identify `SpecGate` artifacts.
const TOOL_NAME: &str = "specgate";

pub(super) struct EncodedBundle {
    pub(super) out: PathBuf,
    pub(super) registry_bytes: Vec<u8>,
    pub(super) trace_bytes: Vec<u8>,
    pub(super) manifest_bytes: Vec<u8>,
    pub(super) report: CaptureReport,
}

pub(super) fn encode_bundle(discovered: &Target, request: &BundleRequest, executed: &ExecutedTests) -> Result<EncodedBundle, ContextError> {
    let resolved = &discovered.target;
    let selected_id = &request.component;
    let selected = selected_id.as_str();
    let captures = filter(&executed.captures, selected_id);
    if captures.is_empty() {
        let failures = if executed.failures.is_empty() {
            String::new()
        } else {
            format!(
                "; these tests failed under capture and were skipped: {}",
                executed
                    .failures
                    .iter()
                    .map(|failure| failure.scenario_name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return Err(ContextError::domain(format!(
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
        let schema = normalize_registry(&discovered.registry, resolved.language, *component)?;
        schemas.push(
            serde_json::to_string(&schema)
                .map_err(|error| ContextError::with_source(format!("failed to serialize normalized discovery schema: {error}"), error))?,
        );
    }
    let registry_id = format!("{REGISTRY_PREFIX}{selected}");
    let schemas = schemas.into_iter().map(specgate_ctsc::registry::Schema::new).collect::<Vec<_>>();
    let registry_encoding = encode_registries(registry_id.clone(), REGISTRY_VERSION.to_string(), &schemas)
        .map_err(|error| ContextError::with_source(format!("failed to encode discovered registry: {error}"), error))?;
    let registry_bytes = registry_encoding.registry_json.into_bytes();
    let registry_digest = sha256_digest(&registry_bytes);

    let operation_count = captures.iter().try_fold(0_usize, |count, capture| {
        count
            .checked_add(capture.operations.len())
            .ok_or_else(|| "captured operation count overflow".to_string())
    })?;
    let metadata = ctsc_capture::Metadata::new(
        env!("CARGO_PKG_VERSION"),
        ctsc_capture::Target::new(resolved.name.as_str(), resolved.language.as_str()),
        ctsc_capture::Registry::new(&registry_id, REGISTRY_VERSION, &registry_digest),
    );
    let trace_encoding = encode_otlp(&captures, &metadata)?;
    let trace_bytes = trace_encoding.otlp_json.into_bytes();
    let trace_digest = sha256_digest(&trace_bytes);
    let scenario_count = captures.len();
    let report_scenarios = u32::try_from(scenario_count).map_err(|_error| "captured scenario count exceeds u32".to_string())?;
    let operations = u32::try_from(operation_count).map_err(|_error| "captured operation count exceeds u32".to_string())?;
    let manifest = CaptureManifest {
        format: MANIFEST_FORMAT,
        format_version: Version::try_from(FORMAT_VERSION).expect("FORMAT_VERSION must be a non-empty manifest version"),
        component_id: selected_id.clone(),
        target: TargetDto {
            name: resolved.name.clone(),
            language: Language::from(resolved.language),
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
        .map_err(|error| ContextError::with_source(format!("failed to serialize capture manifest: {error}"), error))?;
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
