#![cfg(any(test, feature = "test-util"))]

//! Deterministic Rust and C# golden artifact generation and capture-coverage checks.
//!
//! Operational discovery, encoding, capture, parsing, and persistence failures
//! are propagated as [`GoldenError`]; assertions are reserved for matrix shape
//! invariants established by the driver.

// Cohesive implementation group for this subsystem.
use super::*;

/// Build the stable CTSC registry URN for a component.
pub(super) fn registry_id(component: impl AsRef<str>) -> String {
    format!("urn:ctsc:registry:{}", component.as_ref())
}

/// Encode a normalized schema as deterministic compact CTSC registry bytes.
pub(super) fn registry_bytes(schema: &specgate_discovery::schema::Schema, component: impl AsRef<str>) -> Result<Vec<u8>, GoldenError> {
    let schema_json = serde_json::to_string(schema).map_err(|error| GoldenError::wrap("failed to serialize discovery schema", error))?;
    registry_json_bytes(schema_json, component.as_ref())
}

/// Encode the serialized normalized metadata retained by native capture.
fn registry_json_bytes(schema_json: String, component: &str) -> Result<Vec<u8>, GoldenError> {
    let encoded = encode_registry(
        registry_id(component),
        REGISTRY_VERSION.to_string(),
        specgate_ctsc::registry::Schema::new(schema_json),
    )
    .map_err(|error| GoldenError::message(format!("failed to encode CTSC registry: {error}")))?;
    Ok(encoded.registry_json.into_bytes())
}

/// Rust matrix rows grouped by the sources directory their implementation builds.
///
/// One discovery and one batched capture run per group, so every component in a
/// group must resolve to the same source root through the shared binding.
pub(super) fn rust_groups(root: impl AsRef<Path>, matrix: &Matrix) -> Result<BTreeMap<PathBuf, Vec<&Row>>, GoldenError> {
    let binding = repo_path(root.as_ref(), &matrix.binding);
    let mut groups = BTreeMap::<PathBuf, Vec<&Row>>::new();
    for row in matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
    {
        let Some(language) = row.rust.as_ref() else {
            continue;
        };
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let resolved = resolve_implementation(&binding, component, Some(&language.binding))
            .map_err(|error| GoldenError::message(format!("{component}: binding resolution failed: {error}")))?;
        groups.entry(resolved.implementation.sources).or_default().push(row);
    }
    Ok(groups)
}

/// Generate and validate Rust artifacts for one group of rows sharing a sources directory.
pub(super) fn generate_rust(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: &Path,
    rows: &[&Row],
) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let Some(first) = rows.first() else {
        return Ok(());
    };
    let binding = repo_path(root, &matrix.binding);
    let first_language = first.rust.as_ref().expect("grouped Rust matrix row must declare Rust metadata");
    let implementation_label = first_language.binding.as_str();
    let first_component = ComponentId::from(
        first
            .component
            .clone()
            .expect("implementation-component matrix row must declare a component identity"),
    );
    let discovered = discover(&binding, &first_component, Some(implementation_label))
        .map_err(|error| GoldenError::message(format!("{implementation_label}: discovery failed: {}", error.diagnostic())))?;
    let present = discovered.registry.present_components();
    for row in rows {
        let component = row
            .component
            .as_ref()
            .expect("implementation-component matrix row must declare a component identity");
        if !present.iter().any(|candidate| candidate.as_str() == component) {
            return Err(GoldenError::message(format!(
                "{implementation_label}: matrix component '{component}' is absent from discovery; available: {}",
                present
                    .iter()
                    .map(specgate_discovery::identity::ComponentId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }
    }
    let mut unmatched = present
        .iter()
        .filter(|component| !rows.iter().any(|row| row.component.as_deref() == Some(component.as_str())))
        .cloned()
        .collect::<Vec<_>>();
    unmatched.sort();
    if !unmatched.is_empty() {
        return Err(GoldenError::message(format!(
            "{implementation_label}: discovered components missing from the matrix: {}",
            unmatched.join(", ")
        )));
    }
    assert_sync(implementation_label, &discovered.registry, rows)?;

    for row in rows.iter().filter(|row| !row.captures_rust()) {
        let component = row
            .component
            .as_ref()
            .expect("implementation-component matrix row must declare a component identity");
        let schema = normalize_registry(&discovered.registry, discovered.implementation.language(), component.as_str())
            .map_err(|error| GoldenError::message(format!("{component}: discovery-only normalization failed: {error}")))?;
        let bytes = registry_bytes(&schema, component)?;
        publish_discovery_registry(row, &row.rust.as_ref().expect("Rust row").binding, out_root, scratch, &bytes)?;
    }

    let requests = rows
        .iter()
        .filter(|row| row.captures_rust())
        .map(|row| BundleRequest {
            component: ComponentId::from(
                row.component
                    .clone()
                    .expect("implementation-component matrix row must declare a component identity"),
            ),
            out: row.capture_dir(out_root, scratch, &row.rust.as_ref().expect("Rust row").binding),
            excluded_operations: row.capture_exclusions.iter().map(|exclusion| exclusion.operation.clone()).collect(),
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return Ok(());
    }
    let reports = capture_strict(&discovered, &requests)
        .map_err(|error| GoldenError::message(format!("{implementation_label}: batched capture failed: {}", error.diagnostic())))?;
    assert_eq!(
        reports.len(),
        requests.len(),
        "{implementation_label}: capture must return exactly one report per request"
    );
    assert_coverage(implementation_label, &discovered.registry, rows, out_root, scratch)?;
    for row in rows.iter().filter(|row| row.captures_rust()) {
        validate_producer_capture(row, implementation_label, out_root, scratch)?;
    }
    Ok(())
}

/// Named semantic identity used to compare discovery and capture coverage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct OperationIdentity {
    component: ComponentId,
    operation: String,
}
impl OperationIdentity {
    /// Construct an exact component-and-operation identity for set comparison.
    pub(super) fn new(component: impl AsRef<str>, operation: impl Into<String>) -> Self {
        Self {
            component: ComponentId::from(component.as_ref()),
            operation: operation.into(),
        }
    }
}

/// Fail unless every discovered operation is represented by at least one
/// captured span, including operations reached only through nested calls.
///
/// Exclusions are exact semantic operation identities. They must name a real,
/// currently unobserved operation; an unknown or newly covered exclusion is
/// stale and fails rather than weakening the gate indefinitely.
/// Assert that captured operations exactly cover the matrix declarations.
// Missing, unexpected, unknown-exclusion, and stale-exclusion diagnostics can each occur once per row.
const PROBLEM_CAP: usize = 4;

pub(super) fn assert_coverage(
    implementation_label: impl AsRef<str>,
    registry: &Registry,
    rows: &[&Row],
    out_root: impl AsRef<Path>,
    scratch: &Path,
) -> Result<(), GoldenError> {
    let implementation_label = implementation_label.as_ref();
    let out_root = out_root.as_ref();
    let mut problems = Vec::with_capacity(rows.len().saturating_mul(PROBLEM_CAP));
    for row in rows.iter().filter(|row| {
        row.implementation(implementation_label)
            .is_some_and(|(_, implementation)| implementation.phases.contains(&Phase::Capture))
    }) {
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let discovered = registry
            .ops
            .iter()
            .filter(|operation| !operation.is_setup && operation.component == component)
            .map(|operation| OperationIdentity::new(&operation.component, operation.name.to_string()))
            .collect::<BTreeSet<_>>();
        problems.extend(row_coverage(implementation_label, row, &discovered, out_root, scratch)?);
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(GoldenError::message(format!(
            "{implementation_label}: golden capture operation coverage failed:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// Shared exact coverage check for raw Rust and normalized C# discovery.
pub(super) fn row_coverage(
    label: &str,
    row: &Row,
    discovered: &BTreeSet<OperationIdentity>,
    out_root: &Path,
    scratch: &Path,
) -> Result<Vec<String>, GoldenError> {
    let trace = std::fs::File::open(row.capture_dir(out_root, scratch, label).join(trace_file()))
        .map_err(|error| GoldenError::wrap(format!("{} [{label}]: failed to open captured trace", row.id), error))?;
    let captured = captured_ops(trace)?;
    let component = row.component.as_deref().expect("component row");
    let excluded = row
        .capture_exclusions
        .iter()
        .map(|exclusion| OperationIdentity::new(component, exclusion.operation.clone()))
        .collect();
    Ok(coverage_problems(format!("{} [{label}]", row.id), discovered, &captured, &excluded))
}

/// Validate every native producer against its own full bundle registry.
pub(super) fn validate_producer_capture(row: &Row, label: &str, out_root: &Path, scratch: &Path) -> Result<(), GoldenError> {
    let capture = row.capture_dir(out_root, scratch, label);
    let problems = capture_problems(&capture);
    if !problems.is_empty() {
        return Err(GoldenError::message(format!(
            "{} [{label}]: native producer validation failed:\n  {}",
            row.id,
            problems.join("\n  ")
        )));
    }
    Ok(())
}

/// Keep independent discovery metadata transient; only the selected endpoint
/// also publishes the single canonical registry for a discovery-only component.
pub(super) fn publish_discovery_registry(row: &Row, label: &str, out_root: &Path, scratch: &Path, bytes: &[u8]) -> Result<(), GoldenError> {
    write_bytes(row.producer_registry(out_root, scratch, label), bytes)?;
    if row.reference.as_ref().is_some_and(|reference| reference.implementation == label) {
        write_bytes(row.reference_dir(out_root).join(registry_file()), bytes)?;
    }
    Ok(())
}

/// Parse a trace stream and return its unique operation identities.
pub(super) fn captured_ops(reader: impl std::io::Read) -> Result<BTreeSet<OperationIdentity>, GoldenError> {
    let document: serde_json::Value = serde_json::from_reader(reader)
        .map_err(|error| GoldenError::wrap(format!("{} is not valid JSON", trace_file().display()), error))?;
    let resources = document["resourceSpans"]
        .as_array()
        .ok_or_else(|| format!("{} has no resourceSpans array", trace_file().display()))?;
    let mut operations = BTreeSet::new();
    for resource in resources {
        let scopes = resource["scopeSpans"]
            .as_array()
            .ok_or_else(|| format!("{} resource has no scopeSpans array", trace_file().display()))?;
        for scope in scopes {
            let spans = scope["spans"]
                .as_array()
                .ok_or_else(|| format!("{} scope has no spans array", trace_file().display()))?;
            for span in spans.iter().filter(|span| span["name"].as_str() == Some("conformance.operation")) {
                let attributes = span["attributes"]
                    .as_array()
                    .ok_or_else(|| "conformance.operation span has no attributes array".to_string())?;
                let attribute = |key: &str| {
                    attributes
                        .iter()
                        .find(|attribute| attribute["key"].as_str() == Some(key))
                        .and_then(|attribute| attribute["value"]["stringValue"].as_str())
                        .map(str::to_string)
                        .ok_or_else(|| format!("conformance.operation span has no string attribute '{key}'"))
                };
                operations.insert(OperationIdentity::new(
                    attribute("conformance.component.id")?,
                    attribute("conformance.operation.name")?,
                ));
            }
        }
    }
    Ok(operations)
}

/// Return deterministic missing and unexpected operation diagnostics.
pub(super) fn coverage_problems(
    row_id: impl AsRef<str>,
    discovered: &BTreeSet<OperationIdentity>,
    captured: &BTreeSet<OperationIdentity>,
    excluded: &BTreeSet<OperationIdentity>,
) -> Vec<String> {
    let row_id = row_id.as_ref();
    let missing = discovered
        .difference(captured)
        .filter(|operation| !excluded.contains(*operation))
        .map(operation_identity)
        .collect::<Vec<_>>();
    let unexpected = captured.difference(discovered).map(operation_identity).collect::<Vec<_>>();
    let unknown_exclusions = excluded.difference(discovered).map(operation_identity).collect::<Vec<_>>();
    let stale_exclusions = excluded.intersection(captured).map(operation_identity).collect::<Vec<_>>();

    let mut problems = Vec::new();
    if !missing.is_empty() {
        problems.push(format!(
            "{row_id}: discovered operations have no captured trace: [{}]",
            missing.join(", ")
        ));
    }
    if !unexpected.is_empty() {
        problems.push(format!(
            "{row_id}: captured traces contain undiscovered operations: [{}]",
            unexpected.join(", ")
        ));
    }
    if !unknown_exclusions.is_empty() {
        problems.push(format!(
            "{row_id}: capture exclusions name undiscovered operations: [{}]",
            unknown_exclusions.join(", ")
        ));
    }
    if !stale_exclusions.is_empty() {
        problems.push(format!(
            "{row_id}: capture exclusions are stale because the operations were captured: [{}]",
            stale_exclusions.join(", ")
        ));
    }
    problems
}

/// Format an operation identity in its stable diagnostic form.
pub(super) fn operation_identity(identity: &OperationIdentity) -> String {
    format!("{}::{}", identity.component, identity.operation)
}

/// Fail unless every captured Rust component excludes each async setup.
///
/// Async operations are capturable because their futures carry the synchronized
/// collector context. Setup staging remains thread-local, so an async setup
/// would silently omit construction inputs without this preflight rejection.
pub(super) fn assert_sync(implementation_label: impl AsRef<str>, registry: &Registry, rows: &[&Row]) -> Result<(), GoldenError> {
    let implementation_label = implementation_label.as_ref();
    for row in rows.iter().filter(|row| row.captures_rust()) {
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let mut asynchronous = registry
            .ops
            .iter()
            .filter(|candidate| candidate.is_setup && candidate.is_async && candidate.component.as_str() == component)
            .filter(|candidate| {
                !row.capture_exclusions
                    .iter()
                    .any(|exclusion| exclusion.operation == candidate.name.as_str())
            })
            .map(|candidate| format!("setup '{}'", candidate.fn_name))
            .collect::<Vec<_>>();
        asynchronous.sort();
        if !asynchronous.is_empty() {
            return Err(GoldenError::message(format!(
                "row '{}' captures component '{component}', which declares async {}; async setups require exact captureExclusions because setup staging remains thread-local ({implementation_label})",
                row.id,
                asynchronous.join(", ")
            )));
        }
    }
    Ok(())
}

/// Generate and validate C# artifacts for every matrix binding.
pub(super) fn generate_csharp(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: &Path,
) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent && row.csharp.is_some())
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(());
    }
    let implementation_label = rows[0]
        .csharp
        .as_ref()
        .expect("selected C# matrix row must declare C# implementation metadata")
        .binding
        .clone();
    if !rows.iter().all(|row| {
        row.csharp
            .as_ref()
            .expect("selected C# matrix row must declare C# implementation metadata")
            .binding
            == implementation_label
    }) {
        return Err(GoldenError::message("every C# component row must use one implementation label"));
    }
    let binding = repo_path(root, &matrix.binding);
    let components = rows
        .iter()
        .map(|row| {
            row.component
                .as_deref()
                .expect("implementation-component matrix row must declare a component identity")
        })
        .collect::<Vec<_>>();
    let requests = rows
        .iter()
        .filter(|row| {
            row.csharp
                .as_ref()
                .is_some_and(|implementation| implementation.phases.contains(&Phase::Capture))
        })
        .map(|row| BundleRequest {
            component: ComponentId::from(row.component.clone().expect("component row")),
            out: row.capture_dir(out_root, scratch, &implementation_label),
            excluded_operations: row.capture_exclusions.iter().map(|exclusion| exclusion.operation.clone()).collect(),
        })
        .collect::<Vec<_>>();
    let anchor = ComponentId::from(components[0]);
    if requests.is_empty() {
        // Discovery-only bindings may use tests.command and must not pay for
        // weaving or strict native test enumeration. Mixed batches still take
        // the capture path below, retaining its same-woven-assembly metadata.
        let selected = components
            .iter()
            .copied()
            .map(specgate_discovery::identity::ComponentId::from)
            .collect::<Vec<_>>();
        let discovered = discover_batch(&binding, &selected[0], Some(&implementation_label), &selected)
            .map_err(|error| GoldenError::wrap(format!("{implementation_label}: batched discovery failed"), error))?;
        if let Some(diagnostic) = inventory_mismatch(
            &implementation_label,
            &components.iter().map(|component| (*component).to_string()).collect(),
            &discovered.present_components,
        ) {
            return Err(GoldenError::message(diagnostic));
        }
        for row in &rows {
            let component = row.component.as_deref().expect("component row");
            let schema = match discovered.schema(component) {
                SchemaLookup::Found(schema) => schema,
                SchemaLookup::Invalid(error) => {
                    return Err(GoldenError::message(format!(
                        "{component}: discovery-only normalization failed: {error}"
                    )));
                }
                SchemaLookup::Missing => {
                    return Err(GoldenError::message(format!("{component}: missing discovery-only metadata")));
                }
            };
            let bytes = registry_bytes(schema, component)?;
            publish_discovery_registry(row, &implementation_label, out_root, scratch, &bytes)?;
        }
        return Ok(());
    }
    let schemas = capture_csharp_strict(binding, &anchor, Some(&implementation_label), &requests).map_err(|error| {
        GoldenError::message(format!(
            "{implementation_label}: batched native capture failed: {}",
            error.diagnostic()
        ))
    })?;
    if let Some(diagnostic) = inventory_mismatch(
        &implementation_label,
        &components.iter().map(|component| (*component).to_string()).collect(),
        &schemas.keys().collect::<Vec<_>>(),
    ) {
        return Err(GoldenError::message(diagnostic));
    }
    let mut problems = Vec::new();
    for row in &rows {
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let schema = schemas.get(component).expect("exact built metadata inventory");
        if row.csharp.as_ref().expect("C# row").phases.contains(&Phase::Capture) {
            let metadata: serde_json::Value =
                serde_json::from_str(schema).map_err(|error| GoldenError::wrap(format!("{component}: invalid capture metadata"), error))?;
            let operations = metadata["operations"]
                .as_array()
                .ok_or_else(|| GoldenError::message(format!("{component}: metadata has no operations")))?;
            let discovered = operations
                .iter()
                .map(|operation| {
                    operation["name"]
                        .as_str()
                        .map(|name| OperationIdentity::new(component, name))
                        .ok_or_else(|| GoldenError::message(format!("{component}: metadata operation has no name")))
                })
                .collect::<Result<_, _>>()?;
            problems.extend(row_coverage(&implementation_label, row, &discovered, out_root, scratch)?);
            validate_producer_capture(row, &implementation_label, out_root, scratch)?;
        } else {
            let bytes = registry_json_bytes(schema.clone(), component)?;
            publish_discovery_registry(row, &implementation_label, out_root, scratch, &bytes)?;
        }
    }
    if !problems.is_empty() {
        return Err(GoldenError::message(format!(
            "C# golden capture operation coverage failed:\n  {}",
            problems.join("\n  ")
        )));
    }
    Ok(())
}

/// Fail unless the components a target actually declares are exactly the ones
/// the matrix claims. Seeding discovery from matrix rows alone would hide a
/// component that exists in a covered source file but has no row.
/// Assert exact identity inventory equality with a contextual label.
pub(super) fn inventory_mismatch(label: impl AsRef<str>, expected: &BTreeSet<String>, present: &[impl AsRef<str>]) -> Option<String> {
    let label = label.as_ref();
    let present = present
        .iter()
        .map(|component| component.as_ref().to_owned())
        .collect::<BTreeSet<_>>();
    let missing = expected.difference(&present).cloned().collect::<Vec<_>>();
    let extra = present.difference(expected).cloned().collect::<Vec<_>>();
    if missing.is_empty() && extra.is_empty() {
        None
    } else {
        Some(format!(
            "{label}: discovered components disagree with the matrix; absent from discovery: [{}]; declared by no matrix row: [{}]",
            missing.join(", "),
            extra.join(", ")
        ))
    }
}
