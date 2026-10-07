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
    let component = component.as_ref();
    let schema_json = serde_json::to_string(schema).map_err(|error| GoldenError::wrap("failed to serialize discovery schema", error))?;
    let encoded = encode_registry(
        registry_id(component),
        REGISTRY_VERSION.to_string(),
        specgate_ctsc::registry::Schema::new(schema_json),
    )
    .map_err(|error| GoldenError::message(format!("failed to encode CTSC registry: {error}")))?;
    Ok(encoded.registry_json.into_bytes())
}

/// Empty component selection asks discovery to return every component in the binding.
const ALL_COMPONENTS: &str = "";

/// Generate and validate Rust artifacts for one matrix binding group.
pub(super) fn generate_rust(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    binding_key: impl AsRef<str>,
) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let binding_key = binding_key.as_ref();
    let rows = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
        .filter(|row| row.rust.as_ref().is_some_and(|language| language.binding == binding_key))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(());
    }
    let binding = repo_path(root, &matrix.bindings[binding_key]);
    let discovered = discover(&binding, ALL_COMPONENTS)
        .map_err(|error| GoldenError::message(format!("{binding_key}: discovery failed: {}", error.diagnostic())))?;
    let present = discovered.registry.present_components();
    for row in &rows {
        let component = row
            .component
            .as_ref()
            .expect("implementation-component matrix row must declare a component identity");
        if !present.iter().any(|candidate| candidate.as_str() == component) {
            return Err(GoldenError::message(format!(
                "{binding_key}: matrix component '{component}' is absent from discovery; available: {}",
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
            "{binding_key}: discovered components missing from the matrix: {}",
            unmatched.join(", ")
        )));
    }
    assert_sync(binding_key, &discovered.registry, &rows)?;

    for row in rows.iter().filter(|row| !row.captures_rust()) {
        let component = row
            .component
            .as_ref()
            .expect("implementation-component matrix row must declare a component identity");
        let schema = normalize_registry(&discovered.registry, discovered.target.language, component.as_str())
            .map_err(|error| GoldenError::message(format!("{component}: discovery-only normalization failed: {error}")))?;
        let bytes = registry_bytes(&schema, component)?;
        write_bytes(row.rust_dir(out_root).join(registry_file()), &bytes)?;
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
            out: row.rust_dir(out_root),
            excluded_operations: row.capture_exclusions.iter().map(|exclusion| exclusion.operation.clone()).collect(),
        })
        .collect::<Vec<_>>();
    if requests.is_empty() {
        return Ok(());
    }
    let reports = capture_strict(&discovered, &requests)
        .map_err(|error| GoldenError::message(format!("{binding_key}: batched capture failed: {}", error.diagnostic())))?;
    assert_eq!(
        reports.len(),
        requests.len(),
        "{binding_key}: capture must return exactly one report per request"
    );
    assert_coverage(binding_key, &discovered.registry, &rows, out_root)?;
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
    binding_key: impl AsRef<str>,
    registry: &Registry,
    rows: &[&Row],
    out_root: impl AsRef<Path>,
) -> Result<(), GoldenError> {
    let binding_key = binding_key.as_ref();
    let out_root = out_root.as_ref();
    let mut problems = Vec::with_capacity(rows.len().saturating_mul(PROBLEM_CAP));
    for row in rows.iter().filter(|row| row.captures_rust()) {
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
        let trace = std::fs::File::open(row.rust_dir(out_root).join(trace_file()))
            .map_err(|error| GoldenError::wrap(format!("{}: failed to open captured trace", row.id), error))?;
        let captured = captured_ops(trace)
            .map_err(|error| GoldenError::message(format!("{}: failed to inspect captured operation coverage: {error}", row.id)))?;
        let excluded = row
            .capture_exclusions
            .iter()
            .map(|exclusion| OperationIdentity::new(component, exclusion.operation.clone()))
            .collect::<BTreeSet<_>>();
        problems.extend(coverage_problems(row.id.as_str(), &discovered, &captured, &excluded));
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(GoldenError::message(format!(
            "{binding_key}: golden capture operation coverage failed:\n  {}",
            problems.join("\n  ")
        )))
    }
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

/// Fail unless every row that captures a Rust bundle is synchronous or
/// explicitly excludes each asynchronous operation.
///
/// Native capture state is thread-local: an async operation rejects capture
/// from inside its own body and an async setup is not instrumented at all. A
/// row that captures one without an exact exclusion would either fail opaquely
/// or encode a bundle whose input surface is silently incomplete. The matrix
/// is checked here, against real discovery, before capture runs.
/// Assert that synchronous matrix rows match raw registry declarations.
pub(super) fn assert_sync(binding_key: impl AsRef<str>, registry: &Registry, rows: &[&Row]) -> Result<(), GoldenError> {
    let binding_key = binding_key.as_ref();
    for row in rows.iter().filter(|row| row.captures_rust()) {
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let mut asynchronous = registry
            .ops
            .iter()
            .filter(|candidate| candidate.is_async && candidate.component.as_str() == component)
            .filter(|candidate| {
                !row.capture_exclusions
                    .iter()
                    .any(|exclusion| exclusion.operation == candidate.name.as_str())
            })
            .map(|candidate| {
                let kind = if candidate.is_setup { "setup" } else { "operation" };
                format!("{kind} '{}'", candidate.fn_name)
            })
            .collect::<Vec<_>>();
        asynchronous.sort();
        if !asynchronous.is_empty() {
            return Err(GoldenError::message(format!(
                "row '{}' captures component '{component}', which declares async {}; async operations require exact captureExclusions until capture context is task-safe ({binding_key})",
                row.id,
                asynchronous.join(", ")
            )));
        }
    }
    Ok(())
}

/// Generate and validate C# artifacts for every matrix binding.
pub(super) fn generate_csharp(root: impl AsRef<Path>, matrix: &Matrix, out_root: impl AsRef<Path>) -> Result<(), GoldenError> {
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
    let binding_key = rows[0]
        .csharp
        .as_ref()
        .expect("selected C# matrix row must declare C# binding metadata")
        .binding
        .clone();
    if !rows.iter().all(|row| {
        row.csharp
            .as_ref()
            .expect("selected C# matrix row must declare C# binding metadata")
            .binding
            == binding_key
    }) {
        return Err(GoldenError::message("every C# component row must use one binding"));
    }
    let binding = repo_path(root, &matrix.bindings[&binding_key]);
    let components = rows
        .iter()
        .map(|row| {
            row.component
                .as_deref()
                .expect("implementation-component matrix row must declare a component identity")
        })
        .collect::<Vec<_>>();
    let component_ids = components
        .iter()
        .copied()
        .map(specgate_discovery::identity::ComponentId::from)
        .collect::<Vec<_>>();

    let discovered = discover_batch(binding, None, &component_ids)
        .map_err(|error| GoldenError::message(format!("{binding_key}: batched C# discovery failed: {error}")))?;
    if let Some(diagnostic) = inventory_mismatch(
        &binding_key,
        &components.iter().map(|component| (*component).to_string()).collect(),
        &discovered.present_components,
    ) {
        return Err(GoldenError::message(diagnostic));
    }
    for row in &rows {
        let component = row
            .component
            .as_deref()
            .expect("implementation-component matrix row must declare a component identity");
        let schema = match discovered.schema(component) {
            SchemaLookup::Found(schema) => schema,
            SchemaLookup::Invalid(error) => return Err(GoldenError::message(format!("{component}: C# normalization failed: {error}"))),
            SchemaLookup::Missing => return Err(GoldenError::message(format!("{component}: C# discovery returned no metadata"))),
        };
        let bytes = registry_bytes(schema, component)?;
        write_bytes(row.csharp_dir(out_root).join(registry_file()), &bytes)?;
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
