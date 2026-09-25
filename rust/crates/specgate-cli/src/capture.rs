//! `specgate capture <binding.yaml> --out <dir>` — capture passing Rust tests
//! as one deterministic native CTSC reference bundle.

use serde::Serialize;
use sha2::{Digest, Sha256};
use specgate::__rt::{NativeCapture, NativeCaptureConfig, NativeCaptureEnvironmentConfig};
use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::{encode_native_captures_otlp_result, encode_schema_registries_result};
use specgate_discovery::binding::resolve_binding_target;
use specgate_discovery::discovery::{DiscoveredSchema, Registry, TargetDiscovery, cargo_bin, discover_resolved_target, normalize_registry};
use specgate_discovery::profile::{OperationSelector, read_capture_profile, resolve_component};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const REGISTRY_FILE: &str = "registry.ctsc.json";
const TRACE_FILE: &str = "reference.otlp.json";
const MANIFEST_FILE: &str = "manifest.json";
const REGISTRY_VERSION: &str = "0.1.0";
static CAPTURE_SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

/// Summary of a capture run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct CaptureReport {
    /// The CTSC component ids this bundle's selection nominates, sorted and
    /// joined with `,`.
    ///
    /// Component capture always reports exactly one id. A capture profile may
    /// nominate several, so this is deliberately a set rather than a single
    /// component: it is the selection's identity, not the identity of every
    /// component that happens to appear in the exported trace. Component
    /// capture keeps nested foreign operations verbatim, and those components
    /// are declared in the registry without being reported here.
    #[spec_event]
    pub components: String,
    #[spec_event]
    pub scenarios: i32,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub registry_path: String,
    #[spec_event]
    pub trace_path: String,
    #[spec_event]
    pub manifest_path: String,
}

/// Outcome of `capture`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum CaptureOutcome {
    Complete { report: CaptureReport },
    Error { reason: String },
}

impl std::fmt::Display for CaptureOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaptureOutcome::Complete { report } => write!(
                f,
                "Complete(components={}, scenarios={}, operations={}, registry={}, trace={}, manifest={})",
                report.components, report.scenarios, report.operations, report.registry_path, report.trace_path, report.manifest_path
            ),
            CaptureOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

#[derive(Debug)]
struct TestBinary {
    label: String,
    executable: PathBuf,
    is_library: bool,
}

#[derive(Debug)]
struct IsolatedTest {
    scenario_name: String,
    test_name: String,
    executable: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureManifest {
    format: &'static str,
    format_version: &'static str,
    selection: Vec<OperationSelector>,
    target: ManifestTarget,
    tool: ManifestTool,
    registry: ManifestRegistry,
    reference: ManifestReference,
    scenarios: ManifestScenarios,
}

#[derive(Debug, Serialize)]
struct ManifestTarget {
    name: String,
    language: String,
}

#[derive(Debug, Serialize)]
struct ManifestTool {
    name: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ManifestRegistry {
    path: &'static str,
    id: String,
    version: &'static str,
    digest: String,
}

#[derive(Debug, Serialize)]
struct ManifestReference {
    path: &'static str,
    digest: String,
}

#[derive(Debug, Serialize)]
struct ManifestScenarios {
    count: i32,
    names: Vec<String>,
}

/// Capture passing tests for one Rust binding target as a native CTSC bundle.
///
/// Empty `target` selects the binding default. Empty `component` selects the
/// sole discovered component and errors when discovery is ambiguous.
#[must_use]
#[spec_operation("capture")]
pub fn capture(binding: &str, target: &str, component: &str, out: &str) -> CaptureOutcome {
    match capture_result(binding, target, component, out) {
        Ok(report) => CaptureOutcome::Complete { report },
        Err(reason) => CaptureOutcome::Error { reason },
    }
}

/// Capture the exact operation identities selected by a profile.
#[must_use]
pub fn capture_with_profile(binding: &str, target: &str, profile: &str, out: &str) -> CaptureOutcome {
    match capture_profile_result(binding, target, profile, out) {
        Ok(report) => CaptureOutcome::Complete { report },
        Err(reason) => CaptureOutcome::Error { reason },
    }
}

fn capture_result(binding: &str, target: &str, component: &str, out: &str) -> Result<CaptureReport, String> {
    if out.is_empty() {
        return Err("capture requires a non-empty output directory".to_string());
    }
    let discovered = discover_capture_target(binding, target)?;
    let selected = select_component(&discovered.registry, component)?;
    let mut reports = capture_discovered(
        &discovered,
        &[CaptureRequest {
            component: selected,
            out: PathBuf::from(out),
            excluded_operations: BTreeSet::new(),
        }],
    )?;
    Ok(reports.remove(0))
}

fn capture_profile_result(binding: &str, target: &str, profile: &str, out: &str) -> Result<CaptureReport, String> {
    if out.is_empty() {
        return Err("capture requires a non-empty output directory".to_string());
    }
    if profile.is_empty() {
        return Err("capture requires a non-empty profile path".to_string());
    }
    let profile = read_capture_profile(Path::new(profile))?;
    let discovered = discover_capture_target(binding, target)?;
    let selection = profile.resolve(&[&discovered.registry])?;
    reject_async_selection_capture(&discovered.registry, &selection)?;
    let executed = execute_capture_tests(&discovered)?;
    let bundle = encode_selection_bundle(&discovered, &selection, SelectionMode::Profile, PathBuf::from(out), &executed)?;
    let mut reports = write_encoded_bundles(vec![bundle])?;
    Ok(reports.remove(0))
}

/// One requested component bundle within a batched capture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CaptureRequest {
    pub(crate) component: String,
    pub(crate) out: PathBuf,
    /// Golden-only operation exclusions. Product capture always leaves this
    /// empty. This filters candidates for [`reject_async_setup_capture`] only:
    /// it deliberately does not narrow the exported registry or trace, so a
    /// golden-matrix declaration cannot reshape the captured bundle.
    pub(crate) excluded_operations: BTreeSet<String>,
}

/// Capture many components from one Rust binding target in a single pass.
///
/// Discovery, the libtest build, test enumeration, and test execution each
/// happen exactly once for the whole batch; only bundle encoding is per
/// component. [`capture`] is the one-component case of this same path, so
/// batched and single bundles are byte-identical.
///
/// Callers that already hold a [`TargetDiscovery`] should use
/// [`capture_discovered`] directly and pay discovery once for the whole run.
#[cfg(test)]
pub(crate) fn capture_many(binding: &str, target: &str, requests: &[CaptureRequest]) -> Result<Vec<CaptureReport>, String> {
    if requests.is_empty() {
        return Err("capture requires at least one requested component".to_string());
    }
    let discovered = discover_capture_target(binding, target)?;
    let present = discovered.registry.present_components();
    for request in requests {
        if !present.iter().any(|candidate| candidate == &request.component) {
            return Err(format!(
                "component '{}' not found; available components: {}",
                request.component,
                present.join(", ")
            ));
        }
    }
    capture_discovered(&discovered, requests)
}

pub(crate) fn discover_capture_target(binding: &str, target: &str) -> Result<TargetDiscovery, String> {
    let target_name = if target.is_empty() { None } else { Some(target) };
    let resolved = resolve_binding_target(binding, target_name)?;
    if resolved.language != "rust" {
        return Err(format!(
            "capture currently supports only Rust targets; binding language is '{}'",
            resolved.language
        ));
    }
    if !resolved.target.package_root.join("Cargo.toml").is_file() {
        return Err(format!(
            "capture target '{}' is not a Rust package (no Cargo.toml at {})",
            resolved.name,
            resolved.target.package_root.display()
        ));
    }
    discover_resolved_target(resolved, "")
}

/// Capture many components, keeping the tests that pass.
///
/// This is `specgate capture`'s behavior: a failing test contributes no
/// scenario, and a component left with no scenario at all still errors.
pub(crate) fn capture_discovered(discovered: &TargetDiscovery, requests: &[CaptureRequest]) -> Result<Vec<CaptureReport>, String> {
    capture_discovered_with(discovered, requests, false)
}

/// Capture many components, failing on any failing enumerated fixture test.
///
/// Used by the CTSC golden harness, where a fixture test that fails is a
/// product or fixture defect rather than a scenario to skip: the goldens claim
/// to be the corpus's real behavior, so a red test must not be hidden by a
/// sibling test that happens to cover the same component.
#[cfg(test)]
pub(crate) fn capture_discovered_strict(discovered: &TargetDiscovery, requests: &[CaptureRequest]) -> Result<Vec<CaptureReport>, String> {
    capture_discovered_with(discovered, requests, true)
}

fn capture_discovered_with(
    discovered: &TargetDiscovery,
    requests: &[CaptureRequest],
    reject_failed_tests: bool,
) -> Result<Vec<CaptureReport>, String> {
    reject_async_setup_capture(&discovered.registry, requests)?;
    let executed = execute_capture_tests(discovered)?;
    write_capture_bundles(discovered, requests, &executed, reject_failed_tests)
}

/// Build, enumerate, and run every test of the target once under capture.
///
/// This phase is selection-agnostic: one execution pass serves every requested
/// component and every capture profile, and filtering happens afterwards.
fn execute_capture_tests(discovered: &TargetDiscovery) -> Result<ExecutedTests, String> {
    let resolved = &discovered.target;
    let scratch = capture_scratch_dir(&resolved.target.package_root)?;
    let test_binaries = build_test_binaries(&resolved.target.package_root, scratch.as_ref())?;
    let tests = enumerate_tests(&test_binaries)?;
    run_passing_tests(&tests, scratch.as_ref())
}

fn write_capture_bundles(
    discovered: &TargetDiscovery,
    requests: &[CaptureRequest],
    executed: &ExecutedTests,
    reject_failed_tests: bool,
) -> Result<Vec<CaptureReport>, String> {
    if reject_failed_tests {
        reject_failed_test_batch(executed)?;
    }

    let mut encoded = Vec::with_capacity(requests.len());
    for request in requests {
        encoded.push(encode_component_bundle(discovered, request, executed)?);
    }
    write_encoded_bundles(encoded)
}

/// Write every encoded bundle only after all of them encoded successfully.
fn write_encoded_bundles(encoded: Vec<EncodedBundle>) -> Result<Vec<CaptureReport>, String> {
    let mut reports = Vec::with_capacity(encoded.len());
    for bundle in encoded {
        std::fs::create_dir_all(&bundle.out)
            .map_err(|error| format!("failed to create capture output directory {}: {error}", bundle.out.display()))?;
        write_artifact(&bundle.out.join(REGISTRY_FILE), &bundle.registry_bytes)?;
        write_artifact(&bundle.out.join(TRACE_FILE), &bundle.trace_bytes)?;
        write_artifact(&bundle.out.join(MANIFEST_FILE), &bundle.manifest_bytes)?;
        reports.push(bundle.report);
    }
    Ok(reports)
}

#[derive(Debug)]
struct EncodedBundle {
    out: PathBuf,
    registry_bytes: Vec<u8>,
    trace_bytes: Vec<u8>,
    manifest_bytes: Vec<u8>,
    report: CaptureReport,
}

/// How one request turns full native scenarios into an exported trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionMode<'a> {
    /// `--component <id>`: the component's *top-level* operations anchor the
    /// export. A scenario contributes exactly the subtrees rooted at its
    /// top-level operations of this component, kept verbatim — nested calls
    /// into foreign components stay in the trace with their original parents.
    /// Nothing is ever reparented, and a scenario with no top-level operation
    /// of this component contributes nothing.
    ///
    /// The selection here is *implicitly expanded* to every declared non-setup
    /// operation of the component, so an operation no test exercises is
    /// accepted: `specgate capture` is deliberately lenient ("capture what
    /// passes"), and corpus coverage is the golden matrix's job.
    Component(&'a str),
    /// `--profile`: only exactly-selected operation spans survive, and each one
    /// is promoted to its nearest retained selected ancestor, else to the
    /// scenario span. Promotion is intended: the profile nominates the seam.
    ///
    /// The selection here is *user-authored*: every identity was listed
    /// explicitly in the profile's `include`, so one that produced no span is a
    /// typo or a dead selection and is reported rather than silently dropped.
    Profile,
}

fn encode_component_bundle(
    discovered: &TargetDiscovery,
    request: &CaptureRequest,
    executed: &ExecutedTests,
) -> Result<EncodedBundle, String> {
    let selected = request.component.as_str();
    // `excluded_operations` deliberately does not narrow the selection: it is a
    // golden-matrix declaration about async setup capturability only, and must
    // not silently reshape the exported registry or trace.
    let selection = resolve_component(&discovered.registry, selected)?;
    encode_selection_bundle(
        discovered,
        &selection,
        SelectionMode::Component(selected),
        request.out.clone(),
        executed,
    )
}

fn encode_selection_bundle(
    discovered: &TargetDiscovery,
    selection: &[OperationSelector],
    mode: SelectionMode<'_>,
    out_path: PathBuf,
    executed: &ExecutedTests,
) -> Result<EncodedBundle, String> {
    let resolved = &discovered.target;
    let captures = match mode {
        SelectionMode::Component(component) => filter_component_scenarios(&executed.captures, component),
        SelectionMode::Profile => filter_selected_scenarios(&executed.captures, selection)?,
    };
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
        return Err(format!(
            "no passing tests captured any selected operation; add a handwritten test that invokes the selection{failures}"
        ));
    }
    if matches!(mode, SelectionMode::Profile) {
        reject_unexercised_profile_selection(selection, &captures)?;
    }

    // The registry must declare every identity the exported trace actually
    // contains, or CTSC Linked validation rejects the nested foreign spans that
    // component capture keeps verbatim. Declaring the selection as well keeps
    // an unexercised but selected operation in the component's public surface.
    let mut declared = BTreeMap::<&str, BTreeSet<&str>>::new();
    for selector in selection {
        declared.entry(&selector.component).or_default().insert(&selector.operation);
    }
    for capture in &captures {
        for operation in &capture.operations {
            declared
                .entry(operation.component_id.as_str())
                .or_default()
                .insert(operation.operation_name.as_str());
        }
    }
    let mut schemas = Vec::new();
    for (component, operations) in &declared {
        let mut schema = normalize_registry(&discovered.registry, &resolved.language, component)?;
        retain_selected_operations(&mut schema, operations);
        schemas.push(serde_json::to_string(&schema).map_err(|error| format!("failed to serialize normalized discovery schema: {error}"))?);
    }
    let selected_components = selection
        .iter()
        .map(|selector| selector.component.as_str())
        .collect::<BTreeSet<_>>();
    let registry_id = selection_registry_id(&selected_components);
    let registry_encoding = encode_schema_registries_result(registry_id.clone(), REGISTRY_VERSION.to_string(), &schemas)?;
    let registry_bytes = registry_encoding.registry_json.into_bytes();
    let registry_digest = sha256_digest(&registry_bytes);

    let operation_count = captures.iter().try_fold(0_usize, |count, capture| {
        count
            .checked_add(capture.operations.len())
            .ok_or_else(|| "captured operation count overflow".to_string())
    })?;
    let trace_encoding = encode_native_captures_otlp_result(
        &captures,
        env!("CARGO_PKG_VERSION"),
        &resolved.name,
        &resolved.language,
        &registry_id,
        REGISTRY_VERSION,
        &registry_digest,
    )?;
    let trace_bytes = trace_encoding.otlp_json.into_bytes();
    let trace_digest = sha256_digest(&trace_bytes);
    let scenario_count = i32::try_from(captures.len()).map_err(|_error| "captured scenario count exceeds i32".to_string())?;
    let operations = i32::try_from(operation_count).map_err(|_error| "captured operation count exceeds i32".to_string())?;
    let manifest = CaptureManifest {
        format: "specgate.capture-manifest",
        format_version: "0.2.0",
        selection: selection.to_vec(),
        target: ManifestTarget {
            name: resolved.name.clone(),
            language: resolved.language.clone(),
        },
        tool: ManifestTool {
            name: "specgate",
            version: env!("CARGO_PKG_VERSION"),
        },
        registry: ManifestRegistry {
            path: REGISTRY_FILE,
            id: registry_id,
            version: REGISTRY_VERSION,
            digest: registry_digest,
        },
        reference: ManifestReference {
            path: TRACE_FILE,
            digest: trace_digest,
        },
        scenarios: ManifestScenarios {
            count: scenario_count,
            names: captures.iter().map(|capture| capture.scenario_name.clone()).collect(),
        },
    };
    let manifest_bytes = serde_json::to_vec(&manifest).map_err(|error| format!("failed to serialize capture manifest: {error}"))?;
    let out = out_path.display().to_string();
    let components = selected_components.iter().copied().collect::<Vec<_>>().join(",");

    Ok(EncodedBundle {
        out: out_path,
        registry_bytes,
        trace_bytes,
        manifest_bytes,
        report: CaptureReport {
            components,
            scenarios: scenario_count,
            operations,
            registry_path: report_artifact_path(&out, REGISTRY_FILE),
            trace_path: report_artifact_path(&out, TRACE_FILE),
            manifest_path: report_artifact_path(&out, MANIFEST_FILE),
        },
    })
}

/// Derive the deterministic registry id for one selection's component set.
///
/// A single-component selection keeps the historical
/// `urn:ctsc:registry:<component>` id so component bundles never churn. A
/// multi-component profile cannot use any one component's id, so it gets a
/// stable discriminator derived from the sorted selected component set: two
/// distinct profiles therefore never collide on one registry id.
fn selection_registry_id(components: &BTreeSet<&str>) -> String {
    let mut ids = components.iter().copied();
    if let (Some(single), None) = (ids.next(), ids.next()) {
        return format!("urn:ctsc:registry:{single}");
    }
    let joined = components.iter().copied().collect::<Vec<_>>().join("\n");
    let digest = format!("{:x}", Sha256::digest(joined.as_bytes()));
    format!("urn:ctsc:registry:specgate.capture-profile.{}", &digest[..16])
}

/// Narrow a normalized component schema to the operations the bundle declares.
///
/// `selected` is always the union of the selection and every operation with a
/// span in the exported trace, so this never removes an operation the trace
/// still references.
fn retain_selected_operations(schema: &mut DiscoveredSchema, selected: &BTreeSet<&str>) {
    schema.operations.retain(|operation| selected.contains(operation.name.as_str()));
}

/// Filter scenarios for `--component <id>` by *top-level* operation only.
///
/// Each scenario contributes the subtrees rooted at its top-level operations of
/// `component`, verbatim: nested operations from foreign components stay in the
/// trace with their original `parent_span_id`. A foreign top-level operation
/// and its whole subtree are dropped, and a scenario with no top-level
/// operation of `component` contributes nothing.
fn filter_component_scenarios(scenarios: &[NativeCapture], component: &str) -> Vec<NativeCapture> {
    let mut captures = Vec::new();
    for capture in scenarios {
        let mut retained = capture
            .operations
            .iter()
            .filter(|operation| operation.parent_span_id == capture.scenario.span_id && operation.component_id == component)
            .map(|operation| operation.span_id.clone())
            .collect::<BTreeSet<_>>();
        if retained.is_empty() {
            continue;
        }
        // Spans are recorded in completion order, so one pass cannot assume a
        // parent precedes its children; repeat until the subtree closure is
        // stable.
        loop {
            let grown = capture
                .operations
                .iter()
                .filter(|operation| !retained.contains(&operation.span_id) && retained.contains(&operation.parent_span_id))
                .map(|operation| operation.span_id.clone())
                .collect::<Vec<_>>();
            if grown.is_empty() {
                break;
            }
            retained.extend(grown);
        }
        let mut filtered = capture.clone();
        filtered.operations.retain(|operation| retained.contains(&operation.span_id));
        captures.push(filtered);
    }
    captures
}

/// Reject a profile whose explicitly listed identities produced no spans.
///
/// A profile's `include` is user-authored, so an identity that resolved against
/// compiled metadata yet never occurred in any captured scenario is a typo, a
/// dead selection, or a path no test exercises. Without this check such an
/// identity vanishes silently whenever at least one sibling selector did occur.
///
/// This is deliberately **profile-only**. The `--component` shorthand expands
/// implicitly to every declared non-setup operation of the component, where an
/// unexercised operation is normal and accepted: `specgate capture` keeps its
/// "capture what passes" behavior and the golden matrix enforces corpus
/// coverage separately.
///
/// This never replaces the empty-capture error: that one fires first, when *no*
/// selected identity occurred at all.
fn reject_unexercised_profile_selection(selection: &[OperationSelector], captures: &[NativeCapture]) -> Result<(), String> {
    let observed = captures
        .iter()
        .flat_map(|capture| &capture.operations)
        .map(|operation| (operation.component_id.as_str(), operation.operation_name.as_str()))
        .collect::<BTreeSet<_>>();
    let mut unexercised = selection
        .iter()
        .filter(|selector| !observed.contains(&(selector.component.as_str(), selector.operation.as_str())))
        .collect::<Vec<_>>();
    if unexercised.is_empty() {
        return Ok(());
    }
    unexercised.sort();
    let named = unexercised
        .iter()
        .map(|selector| format!("{}::{}", selector.component, selector.operation))
        .collect::<Vec<_>>()
        .join(", ");
    Err(format!(
        "capture profile selects {} operation(s) that no passing test invoked: {named}; remove them from the profile or add a test that invokes them",
        unexercised.len()
    ))
}

/// Filter scenarios for `--profile` by exact operation identity.
///
/// Only exactly-selected spans survive, and each retained span is reparented to
/// its nearest retained selected ancestor, else to the scenario span. Promotion
/// is intended here: the profile nominates the seam, so an unselected
/// intermediate operation must not remain as a parent.
fn filter_selected_scenarios(scenarios: &[NativeCapture], selection: &[OperationSelector]) -> Result<Vec<NativeCapture>, String> {
    let selected = selection
        .iter()
        .map(|selector| (selector.component.as_str(), selector.operation.as_str()))
        .collect::<BTreeSet<_>>();
    let mut captures = Vec::new();
    for capture in scenarios {
        let retained_ids = capture
            .operations
            .iter()
            .filter(|operation| selected.contains(&(operation.component_id.as_str(), operation.operation_name.as_str())))
            .map(|operation| operation.span_id.as_str())
            .collect::<BTreeSet<_>>();
        if retained_ids.is_empty() {
            continue;
        }
        let parents = capture
            .operations
            .iter()
            .map(|operation| (operation.span_id.as_str(), operation.parent_span_id.as_str()))
            .collect::<BTreeMap<_, _>>();
        let mut filtered = capture.clone();
        filtered
            .operations
            .retain(|operation| retained_ids.contains(operation.span_id.as_str()));
        for operation in &mut filtered.operations {
            let mut parent = operation.parent_span_id.as_str();
            while parent != capture.scenario.span_id && !retained_ids.contains(parent) {
                parent = parents.get(parent).copied().ok_or_else(|| {
                    format!(
                        "captured scenario '{}' operation '{}' has unresolved parent span '{}'",
                        capture.scenario_name, operation.operation_name, parent
                    )
                })?;
            }
            operation.parent_span_id = parent.to_string();
        }
        captures.push(filtered);
    }
    Ok(captures)
}

fn select_component(registry: &Registry, component: &str) -> Result<String, String> {
    let components = registry.present_components();
    if component.is_empty() {
        return match components.len() {
            0 => Err("no components found: target has no annotated operations or types".to_string()),
            1 => Ok(components[0].clone()),
            _ => Err(format!(
                "multiple components present ({}); select one with --component <id>",
                components.join(", ")
            )),
        };
    }
    if components.iter().any(|candidate| candidate == component) {
        Ok(component.to_string())
    } else {
        Err(format!(
            "component '{component}' not found; available components: {}",
            components.join(", ")
        ))
    }
}

/// Reject a capture batch that selects a component with an async setup.
///
/// An async `#[spec_operation]` rejects native capture from inside its own
/// body, but an async `#[spec_setup]` is deliberately left uninstrumented:
/// capture state is thread-local and cannot follow a future across executor
/// threads. Capturing such a component would therefore succeed while silently
/// dropping the setup's construction inputs, encoding a bundle that misstates
/// the component's public input surface. The whole component is rejected here
/// instead — before any test binary is built, run, or encoded — so the failure
/// names the setup rather than surfacing as a missing input much later.
fn reject_async_setup_capture(registry: &Registry, requests: &[CaptureRequest]) -> Result<(), String> {
    for request in requests {
        let component = request.component.as_str();
        let mut asynchronous = registry
            .ops
            .iter()
            .filter(|candidate| candidate.is_setup && candidate.is_async && candidate.component == component)
            .filter(|candidate| !request.excluded_operations.contains(&candidate.name))
            .collect::<Vec<_>>();
        asynchronous.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.fn_name.cmp(&right.fn_name)));
        if let Some(setup) = asynchronous.first() {
            return Err(format!(
                "component '{component}' declares async setup '{}' for operation '{component}::{}'; native capture cannot instrument an async setup, so this component is discovery-only until capture context is task-safe",
                setup.fn_name, setup.name
            ));
        }
    }
    Ok(())
}

fn reject_async_selection_capture(registry: &Registry, selection: &[OperationSelector]) -> Result<(), String> {
    let selected = selection
        .iter()
        .map(|selector| (selector.component.as_str(), selector.operation.as_str()))
        .collect::<BTreeSet<_>>();
    let mut asynchronous = registry
        .ops
        .iter()
        .filter(|candidate| candidate.is_setup && candidate.is_async)
        .filter(|candidate| selected.contains(&(candidate.component.as_str(), candidate.name.as_str())))
        .collect::<Vec<_>>();
    asynchronous.sort_by(|left, right| {
        left.component
            .cmp(&right.component)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.fn_name.cmp(&right.fn_name))
    });
    if let Some(setup) = asynchronous.first() {
        return Err(format!(
            "selected operation '{}::{}' declares async setup '{}'; native capture cannot instrument an async setup until capture context is task-safe",
            setup.component, setup.name, setup.fn_name
        ));
    }
    Ok(())
}

fn capture_scratch_dir(package_root: &Path) -> Result<specgate_discovery::support::InvocationCache, String> {
    let package_name = package_root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("could not derive package name from {}", package_root.display()))?;
    let invocation = CAPTURE_SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    specgate_discovery::support::InvocationCache::create("capture", package_name, invocation)
}

fn build_test_binaries(package_root: &Path, scratch: &Path) -> Result<Vec<TestBinary>, String> {
    let mut command = Command::new(cargo_bin());
    command.arg("test").arg("--no-run").arg("--quiet").arg("--message-format=json");
    command.current_dir(package_root);
    command.env_remove("RUSTC_WORKSPACE_WRAPPER");
    command.env_remove("CARGO");
    command.env_remove("CARGO_MANIFEST_DIR");
    command.env("CARGO_TARGET_DIR", scratch.join("target"));

    let output = command
        .output()
        .map_err(|error| format!("failed to build capture test binaries: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "capture test build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let mut binaries = Vec::new();
    let mut seen = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact")
            || !message
                .get("profile")
                .and_then(|profile| profile.get("test"))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        {
            continue;
        }
        let Some(executable) = message.get("executable").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !seen.insert(executable.to_string()) {
            continue;
        }
        let target = &message["target"];
        let label = target["name"].as_str().unwrap_or("test").to_string();
        let is_library = target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("lib")));
        binaries.push(TestBinary {
            label,
            executable: PathBuf::from(executable),
            is_library,
        });
    }
    binaries.sort_by(|left, right| {
        right
            .is_library
            .cmp(&left.is_library)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.executable.cmp(&right.executable))
    });
    if binaries.is_empty() {
        return Err("capture test build produced no libtest binaries".to_string());
    }
    Ok(binaries)
}

fn enumerate_tests(binaries: &[TestBinary]) -> Result<Vec<IsolatedTest>, String> {
    let mut tests = Vec::new();
    for binary in binaries {
        let output = Command::new(&binary.executable)
            .arg("--list")
            .arg("--format")
            .arg("terse")
            .output()
            .map_err(|error| format!("failed to list tests in {}: {error}", binary.executable.display()))?;
        if !output.status.success() {
            return Err(format!(
                "capture test enumeration failed for {}: {}",
                binary.executable.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let Some(test_name) = line.strip_suffix(": test") else {
                continue;
            };
            let test_name = test_name.trim().to_string();
            let scenario_name = if binary.is_library {
                test_name.clone()
            } else {
                format!("{}::{test_name}", binary.label)
            };
            tests.push(IsolatedTest {
                scenario_name,
                test_name,
                executable: binary.executable.clone(),
            });
        }
    }
    tests.sort_by(|left, right| left.scenario_name.cmp(&right.scenario_name));
    Ok(tests)
}

/// Outcome of one capture execution pass over every enumerated test.
#[derive(Debug, Default)]
struct ExecutedTests {
    captures: Vec<NativeCapture>,
    failures: Vec<FailedTest>,
}

/// One enumerated test that failed while running under capture.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FailedTest {
    scenario_name: String,
    /// Exit status plus the tail of the test's own output. Deliberately not an
    /// artifact: it is machine-specific, and only ever reaches an error string.
    summary: String,
}

/// Reject a capture batch in which any enumerated test failed.
fn reject_failed_test_batch(executed: &ExecutedTests) -> Result<(), String> {
    if executed.failures.is_empty() {
        return Ok(());
    }
    let detail = executed
        .failures
        .iter()
        .map(|failure| format!("{}: {}", failure.scenario_name, failure.summary))
        .collect::<Vec<_>>()
        .join("\n  ");
    Err(format!(
        "{} fixture test(s) failed under capture; every enumerated test must pass:\n  {detail}",
        executed.failures.len()
    ))
}

/// Summarize one failed test run: its exit status and the tail of each stream.
fn failure_summary(exit_code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let mut parts = vec![exit_code.map_or_else(|| "terminated without an exit code".to_string(), |code| format!("exit code {code}"))];
    for (label, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        let text = String::from_utf8_lossy(bytes);
        let mut tail = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .rev()
            .take(12)
            .collect::<Vec<_>>();
        tail.reverse();
        if tail.is_empty() {
            continue;
        }
        let mut joined = tail.join(" | ");
        if joined.chars().count() > 1_200 {
            joined = joined.chars().take(1_200).collect::<String>() + "...";
        }
        parts.push(format!("{label}: {joined}"));
    }
    parts.join("; ")
}

/// Run every enumerated test once under deterministic native capture and return
/// the sidecars that passing tests produced, in stable scenario order.
///
/// Tests are component-agnostic here: one execution pass serves every requested
/// component, and scenario selection happens afterwards.
fn run_passing_tests(tests: &[IsolatedTest], scratch: &Path) -> Result<ExecutedTests, String> {
    let sidecars = scratch.join("sidecars");
    std::fs::create_dir_all(&sidecars)
        .map_err(|error| format!("failed to create capture sidecar directory {}: {error}", sidecars.display()))?;
    let mut captures = Vec::new();
    let mut failures = Vec::new();
    for (index, test) in tests.iter().enumerate() {
        let sidecar = sidecars.join(format!("{index:08}.json"));
        let _ = std::fs::remove_file(&sidecar);
        let environment = NativeCaptureEnvironmentConfig {
            capture: NativeCaptureConfig {
                scenario_name: test.scenario_name.clone(),
                trace_id: "11111111111111111111111111111111".to_string(),
                run_span_id: "1111111111111101".to_string(),
                scenario_span_id: "1111111111111102".to_string(),
                operation_span_ids: Vec::new(),
                start_time_unix_nano: 0,
                clock_step_unix_nano: 1,
            },
            sidecar_path: sidecar.clone(),
        };
        let environment_json =
            serde_json::to_string(&environment).map_err(|error| format!("failed to serialize native capture environment: {error}"))?;
        let output = Command::new(&test.executable)
            .arg(&test.test_name)
            .arg("--exact")
            .arg("--test-threads=1")
            .env("SPECGATE_NATIVE_CAPTURE", environment_json)
            .output()
            .map_err(|error| format!("failed to run isolated test '{}': {error}", test.scenario_name))?;
        if !output.status.success() {
            let _ = std::fs::remove_file(&sidecar);
            failures.push(FailedTest {
                scenario_name: test.scenario_name.clone(),
                summary: failure_summary(output.status.code(), &output.stdout, &output.stderr),
            });
            continue;
        }
        if !sidecar.is_file() {
            continue;
        }
        let bytes =
            std::fs::read(&sidecar).map_err(|error| format!("failed to read native capture sidecar {}: {error}", sidecar.display()))?;
        let _ = std::fs::remove_file(&sidecar);
        let capture: NativeCapture = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse native capture sidecar for '{}': {error}", test.scenario_name))?;
        if capture.operations.is_empty() {
            continue;
        }
        captures.push(capture);
    }
    Ok(ExecutedTests { captures, failures })
}
fn write_artifact(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|error| format!("failed to write capture artifact {}: {error}", path.display()))
}

fn sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn report_artifact_path(out: &str, filename: &str) -> String {
    let separator = if out.contains('\\') && !out.contains('/') {
        '\\'
    } else if out.contains('/') {
        '/'
    } else {
        std::path::MAIN_SEPARATOR
    };
    let trimmed = out.trim_end_matches(['/', '\\']);
    format!("{trimmed}{separator}{filename}")
}

/// Format a capture outcome for CLI display.
#[must_use]
pub fn format_outcome(outcome: &CaptureOutcome) -> String {
    format!("{outcome}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use specgate_ctsc::validation::{validate_bundle, validate_linked};
    use specgate_discovery::discovery::discover_many_target;

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
            .expect("repository root")
            .to_path_buf()
    }

    fn focused_rust_binding() -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join("rust.binding.yaml")
    }

    fn focused_csharp_binding() -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join("csharp.binding.yaml")
    }

    fn output_dir(label: &str) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-capture-test-{label}-{}", std::process::id()))
    }

    #[test]
    fn capture_stateless_bundle_is_linked_and_byte_identical() {
        let root = output_dir("focused");
        let _ = std::fs::remove_dir_all(&root);
        let requests = vec![
            request_at("fixture.cli.replay", root.join("batch-replay")),
            request_at("fixture.cli.replay", root.join("repeat-replay")),
            request_at("fixture.cli.setup", root.join("setup")),
            request_at("fixture.cli.multiple", root.join("multiple")),
        ];
        let discovered = discover_capture_target(focused_rust_binding().to_str().unwrap(), "").expect("focused fixture discovery");
        let executed = execute_capture_tests(&discovered).expect("focused fixture execution");
        assert_eq!(executed.failures.len(), 1);
        assert_eq!(executed.failures[0].scenario_name, "tests::deliberately_fails_for_strict_capture");

        let reports = write_capture_bundles(&discovered, &requests, &executed, false).expect("batched capture encoding");
        assert_eq!(reports.len(), requests.len());
        assert_eq!(
            reports[0],
            CaptureReport {
                components: "fixture.cli.replay".to_string(),
                scenarios: 2,
                operations: 2,
                registry_path: requests[0].out.join(REGISTRY_FILE).display().to_string(),
                trace_path: requests[0].out.join(TRACE_FILE).display().to_string(),
                manifest_path: requests[0].out.join(MANIFEST_FILE).display().to_string(),
            }
        );
        assert_eq!(reports[2].scenarios, 1);
        assert_eq!(reports[2].operations, 1);
        assert_eq!(reports[3].scenarios, 1);
        assert_eq!(reports[3].operations, 1);

        let batch_dir = &requests[0].out;
        let repeat_dir = &requests[1].out;
        assert_eq!(directory_file_names(batch_dir), vec![MANIFEST_FILE, TRACE_FILE, REGISTRY_FILE]);
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(batch_dir.join(filename)).unwrap(),
                std::fs::read(repeat_dir.join(filename)).unwrap(),
                "{filename} must be byte-identical across repeated capture"
            );
        }

        let registry = std::fs::read(batch_dir.join(REGISTRY_FILE)).unwrap();
        let trace = std::fs::read(batch_dir.join(TRACE_FILE)).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(batch_dir.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(manifest["format"], "specgate.capture-manifest");
        assert_eq!(manifest["formatVersion"], "0.2.0");
        assert_eq!(
            manifest["selection"],
            serde_json::json!([
                {"component":"fixture.cli.replay","operation":"add"},
                {"component":"fixture.cli.replay","operation":"echo"}
            ])
        );
        assert_eq!(manifest["target"], serde_json::json!({"name":"default","language":"rust"}));
        assert_eq!(
            manifest["tool"],
            serde_json::json!({"name":"specgate","version":env!("CARGO_PKG_VERSION")})
        );
        assert_eq!(manifest["registry"]["path"], REGISTRY_FILE);
        assert_eq!(manifest["registry"]["id"], "urn:ctsc:registry:fixture.cli.replay");
        assert_eq!(manifest["registry"]["version"], REGISTRY_VERSION);
        assert_eq!(manifest["registry"]["digest"], sha256_digest(&registry));
        assert_eq!(manifest["reference"]["path"], TRACE_FILE);
        assert_eq!(manifest["reference"]["digest"], sha256_digest(&trace));
        assert_eq!(manifest["scenarios"]["count"], 2);
        assert_eq!(
            manifest["scenarios"]["names"],
            serde_json::json!(["tests::adds_two_and_three", "tests::echoes_all_rust_string_escape_classes"])
        );

        // `capture` and `capture_many` differ only in request cardinality after
        // this shared execution phase. Compare both encodings without rebuilding
        // or rerunning the real fixture toolchain.
        let single = request_at("fixture.cli.replay", root.join("single-replay"));
        write_capture_bundles(&discovered, std::slice::from_ref(&single), &executed, false).expect("single capture encoding");
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(batch_dir.join(filename)).unwrap(),
                std::fs::read(single.out.join(filename)).unwrap(),
                "{filename} must be byte-identical between batched and single capture"
            );
        }

        for request in &requests[0..4] {
            let linked = validate_linked(&request.out.join(TRACE_FILE), &request.out.join(REGISTRY_FILE), &[]);
            assert!(linked.valid, "{} linked validation failed: {:#?}", request.component, linked.issues);
            let bundle = validate_bundle(&request.out);
            assert!(bundle.valid, "{} bundle validation failed: {:#?}", request.component, bundle.issues);
        }

        let setup = read_trace(&requests[2].out);
        assert_eq!(
            operation_inputs(&setup, "increment"),
            vec![("initial".to_string(), serde_json::json!({ "intValue": "4" }))],
            "setup construction input must be folded into the operation's public input surface"
        );

        let multiple_registry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(requests[3].out.join(REGISTRY_FILE)).unwrap()).unwrap();
        assert_eq!(
            multiple_registry["components"][0]["operations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|operation| operation["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["unexercised", "used"]
        );
        let multiple_trace = read_trace(&requests[3].out);
        assert_eq!(operation_spans(&multiple_trace, "used").len(), 1);
        assert!(operation_spans(&multiple_trace, "unexercised").is_empty());

        // Component mode is deliberately lenient about an implicitly expanded
        // operation that no test exercises, and the zero-occurrence diagnostic
        // must not leak into it. The very same identity set fails under profile
        // mode, so the difference is provenance, not the identities themselves.
        let selector = |component: &str, operation: &str| OperationSelector {
            component: component.to_string(),
            operation: operation.to_string(),
        };
        let multiple_selection = resolve_component(&discovered.registry, "fixture.cli.multiple").unwrap();
        assert_eq!(
            multiple_selection,
            vec![
                selector("fixture.cli.multiple", "unexercised"),
                selector("fixture.cli.multiple", "used"),
            ]
        );
        assert!(
            encode_selection_bundle(
                &discovered,
                &multiple_selection,
                SelectionMode::Component("fixture.cli.multiple"),
                root.join("lenient-component"),
                &executed,
            )
            .is_ok(),
            "component mode must not emit the zero-occurrence selection diagnostic"
        );
        let dead = encode_selection_bundle(
            &discovered,
            &multiple_selection,
            SelectionMode::Profile,
            root.join("dead-selector"),
            &executed,
        )
        .expect_err("a profile naming an operation no test invoked must fail");
        assert_eq!(
            dead,
            "capture profile selects 1 operation(s) that no passing test invoked: \
             fixture.cli.multiple::unexercised; remove them from the profile or add a test that invokes them"
        );

        // Several dead selectors are all reported, sorted by component then
        // operation, and the still-live selector is not named.
        let mut mixed_selection = vec![
            selector("fixture.cli.unused", "never_called"),
            selector("fixture.cli.multiple", "used"),
            selector("fixture.cli.multiple", "unexercised"),
            selector("fixture.cli.profile_bridge", "bridge"),
        ];
        mixed_selection.sort();
        let dead_many = encode_selection_bundle(
            &discovered,
            &mixed_selection,
            SelectionMode::Profile,
            root.join("dead-selectors"),
            &executed,
        )
        .expect_err("every dead selector must be reported");
        assert_eq!(
            dead_many,
            "capture profile selects 2 operation(s) that no passing test invoked: \
             fixture.cli.multiple::unexercised, fixture.cli.unused::never_called; \
             remove them from the profile or add a test that invokes them"
        );
        assert!(!root.join("dead-selector").exists() && !root.join("dead-selectors").exists());

        // A profile whose selectors all occur is unaffected by the diagnostic;
        // the empty-capture error remains the distinct, earlier failure.
        let all_dead = encode_selection_bundle(
            &discovered,
            std::slice::from_ref(&selector("fixture.cli.unused", "never_called")),
            SelectionMode::Profile,
            root.join("all-dead"),
            &executed,
        )
        .expect_err("a profile whose every selector is dead still reports the empty capture");
        assert!(
            all_dead.starts_with("no passing tests captured any selected operation"),
            "{all_dead}"
        );

        let profile_selection = vec![
            OperationSelector {
                component: "fixture.cli.profile_leaf".to_string(),
                operation: "leaf".to_string(),
            },
            OperationSelector {
                component: "fixture.cli.profile_root".to_string(),
                operation: "root".to_string(),
            },
        ];
        // Every selector below occurs, so the zero-occurrence diagnostic stays
        // silent and the bundle encodes normally.
        let profile = encode_selection_bundle(
            &discovered,
            &profile_selection,
            SelectionMode::Profile,
            root.join("profile"),
            &executed,
        )
        .expect("profile bundle encoding");
        std::fs::create_dir_all(&profile.out).unwrap();
        write_artifact(&profile.out.join(REGISTRY_FILE), &profile.registry_bytes).unwrap();
        write_artifact(&profile.out.join(TRACE_FILE), &profile.trace_bytes).unwrap();
        write_artifact(&profile.out.join(MANIFEST_FILE), &profile.manifest_bytes).unwrap();
        let profile_trace = read_trace(&profile.out);
        assert!(operation_spans(&profile_trace, "bridge").is_empty());
        let root_ids = span_ids(&operation_spans(&profile_trace, "root"));
        let scenario_ids = scenario_span_ids(&profile_trace);
        assert!(
            operation_spans(&profile_trace, "leaf")
                .iter()
                .all(|span| root_ids.contains(&&span["parentSpanId"]) || scenario_ids.contains(&&span["parentSpanId"])),
            "profile mode promotes each retained span to its nearest retained selected ancestor, else the scenario"
        );
        let profile_manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(profile.out.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(profile_manifest["selection"], serde_json::to_value(&profile_selection).unwrap());
        let linked = validate_linked(&profile.out.join(TRACE_FILE), &profile.out.join(REGISTRY_FILE), &[]);
        assert!(linked.valid, "profile linked validation failed: {:#?}", linked.issues);
        let bundle = validate_bundle(&profile.out);
        assert!(bundle.valid, "profile bundle validation failed: {:#?}", bundle.issues);

        let original_manifest = std::fs::read(profile.out.join(MANIFEST_FILE)).unwrap();
        let mut unknown: serde_json::Value = serde_json::from_slice(&original_manifest).unwrap();
        unknown["unknown"] = serde_json::json!(true);
        std::fs::write(profile.out.join(MANIFEST_FILE), serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(!validate_bundle(&profile.out).valid, "unknown manifest fields must be rejected");
        std::fs::write(profile.out.join(MANIFEST_FILE), &original_manifest).unwrap();

        let source = executed
            .captures
            .iter()
            .find(|capture| capture.scenario_name.contains("captures_profile_ancestry"))
            .expect("nested profile scenario");
        let promoted = filter_selected_scenarios(std::slice::from_ref(source), &profile_selection).unwrap();
        assert!(
            promoted[0].operations.iter().all(|operation| operation.operation_name != "bridge"),
            "an unselected intermediate operation must not survive profile filtering"
        );
        let promoted_root = promoted[0]
            .operations
            .iter()
            .find(|operation| operation.operation_name == "root")
            .expect("selected root span");
        let promoted_leaf = promoted[0]
            .operations
            .iter()
            .find(|operation| operation.operation_name == "leaf")
            .expect("selected leaf span");
        assert_eq!(promoted_leaf.parent_span_id, promoted_root.span_id);
        let leaf_only = filter_selected_scenarios(std::slice::from_ref(source), &profile_selection[..1]).unwrap();
        assert_eq!(leaf_only[0].operations[0].parent_span_id, leaf_only[0].scenario.span_id);

        // Component mode filters TOP-LEVEL operations only. The selected
        // component's top-level subtrees are exported verbatim — nested foreign
        // operations keep their original parents — and a foreign top-level
        // operation is dropped together with its whole subtree.
        let component_root = request_at("fixture.cli.profile_root", root.join("component-root"));
        let root_reports = write_capture_bundles(&discovered, std::slice::from_ref(&component_root), &executed, false)
            .expect("component capture over a multi-component call chain");
        assert_eq!(root_reports[0].components, "fixture.cli.profile_root");
        let root_trace = read_trace(&component_root.out);
        let root_spans = operation_spans(&root_trace, "root");
        let bridge_spans = operation_spans(&root_trace, "bridge");
        let leaf_spans = operation_spans(&root_trace, "leaf");
        assert_eq!(root_spans.len(), 2, "both passing scenarios invoke root at top level");
        assert_eq!(bridge_spans.len(), 2, "nested foreign operations are kept verbatim");
        assert_eq!(leaf_spans.len(), 2, "only the leaf reached through root survives");
        assert!(
            operation_spans(&root_trace, "sibling").is_empty(),
            "a foreign top-level operation and its subtree are dropped"
        );
        let retained_root_ids = span_ids(&root_spans);
        let retained_bridge_ids = span_ids(&bridge_spans);
        assert!(
            bridge_spans.iter().all(|span| retained_root_ids.contains(&&span["parentSpanId"])),
            "component mode never reparents a nested span"
        );
        assert!(
            leaf_spans.iter().all(|span| retained_bridge_ids.contains(&&span["parentSpanId"])),
            "component mode never reparents a nested span"
        );
        let root_registry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(component_root.out.join(REGISTRY_FILE)).unwrap()).unwrap();
        assert_eq!(root_registry["registryId"], "urn:ctsc:registry:fixture.cli.profile_root");
        assert_eq!(
            root_registry["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|component| {
                    (
                        component["id"].as_str().unwrap(),
                        component["operations"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|operation| operation["name"].as_str().unwrap())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("fixture.cli.profile_bridge", vec!["bridge"]),
                ("fixture.cli.profile_leaf", vec!["leaf"]),
                ("fixture.cli.profile_root", vec!["root"]),
            ],
            "the registry must declare every component and operation present in the exported trace"
        );
        let linked = validate_linked(&component_root.out.join(TRACE_FILE), &component_root.out.join(REGISTRY_FILE), &[]);
        assert!(linked.valid, "component linked validation failed: {:#?}", linked.issues);
        let bundle = validate_bundle(&component_root.out);
        assert!(bundle.valid, "component bundle validation failed: {:#?}", bundle.issues);

        let component_sibling = request_at("fixture.cli.profile_sibling", root.join("component-sibling"));
        write_capture_bundles(&discovered, std::slice::from_ref(&component_sibling), &executed, false)
            .expect("the sibling's own top-level subtree is capturable");
        let sibling_trace = read_trace(&component_sibling.out);
        assert!(operation_spans(&sibling_trace, "root").is_empty());
        assert!(operation_spans(&sibling_trace, "bridge").is_empty());
        let sibling_spans = operation_spans(&sibling_trace, "sibling");
        let sibling_leaf = operation_spans(&sibling_trace, "leaf");
        assert_eq!(sibling_spans.len(), 1);
        assert_eq!(sibling_leaf.len(), 1);
        assert_eq!(sibling_leaf[0]["parentSpanId"], sibling_spans[0]["spanId"]);

        // `leaf` is only ever reached as a nested callee, so no scenario has a
        // top-level `leaf` operation and the component contributes nothing.
        let component_leaf = request_at("fixture.cli.profile_leaf", root.join("component-leaf"));
        let leaf_error = write_capture_bundles(&discovered, std::slice::from_ref(&component_leaf), &executed, false)
            .expect_err("a component that is never top-level contributes no scenario");
        assert!(
            leaf_error.contains("no passing tests captured any selected operation"),
            "{leaf_error}"
        );
        assert!(!component_leaf.out.exists());

        // `excluded_operations` is an async-setup screening input only: it must
        // not prune the exported registry or trace.
        let mut excluded = request_at("fixture.cli.multiple", root.join("excluded"));
        excluded.excluded_operations.insert("used".to_string());
        excluded.excluded_operations.insert("unexercised".to_string());
        write_capture_bundles(&discovered, std::slice::from_ref(&excluded), &executed, false)
            .expect("capture exclusions never reshape the bundle");
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(requests[3].out.join(filename)).unwrap(),
                std::fs::read(excluded.out.join(filename)).unwrap(),
                "captureExclusions must not change {filename}"
            );
        }

        let unused = request_at("fixture.cli.unused", root.join("unused"));
        let no_match = write_capture_bundles(&discovered, std::slice::from_ref(&unused), &executed, false)
            .expect_err("an unexercised component must be rejected");
        assert!(no_match.contains("no passing tests captured any selected operation"));
        assert!(!unused.out.exists());

        let strict = request_at("fixture.cli.replay", root.join("strict"));
        let strict_error = write_capture_bundles(&discovered, std::slice::from_ref(&strict), &executed, true)
            .expect_err("strict capture must reject the focused failing scenario");
        assert!(strict_error.starts_with("1 fixture test(s) failed under capture"));
        assert!(strict_error.contains("tests::deliberately_fails_for_strict_capture"));
        assert!(!strict.out.exists());

        let _ = std::fs::remove_dir_all(root);
    }

    fn attribute<'a>(span: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
        span["attributes"]
            .as_array()?
            .iter()
            .find(|attribute| attribute["key"] == key)
            .map(|attribute| &attribute["value"])
    }

    fn operation_spans<'a>(trace: &'a serde_json::Value, operation: &str) -> Vec<&'a serde_json::Value> {
        trace["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .expect("captured spans")
            .iter()
            .filter(|span| attribute(span, "conformance.operation.name").and_then(|value| value["stringValue"].as_str()) == Some(operation))
            .collect()
    }

    fn span_ids<'a>(spans: &[&'a serde_json::Value]) -> Vec<&'a serde_json::Value> {
        spans.iter().map(|span| &span["spanId"]).collect()
    }

    fn scenario_span_ids(trace: &serde_json::Value) -> Vec<&serde_json::Value> {
        trace["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .expect("captured spans")
            .iter()
            .filter(|span| span["name"] == "conformance.scenario")
            .map(|span| &span["spanId"])
            .collect()
    }

    fn operation_inputs(trace: &serde_json::Value, operation: &str) -> Vec<(String, serde_json::Value)> {
        let spans = operation_spans(trace, operation);
        assert!(!spans.is_empty(), "no captured span for operation '{operation}'");
        spans
            .iter()
            .flat_map(|span| {
                attribute(span, "conformance.operation.inputs")
                    .and_then(|value| value["kvlistValue"]["values"].as_array().cloned())
                    .unwrap_or_default()
            })
            .map(|entry| (entry["key"].as_str().unwrap_or_default().to_string(), entry["value"].clone()))
            .collect()
    }

    fn read_trace(bundle: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(bundle.join(TRACE_FILE)).expect("captured trace")).expect("valid OTLP JSON")
    }

    #[test]
    fn capture_errors_are_actionable() {
        assert_eq!(
            capture_many(focused_rust_binding().to_str().unwrap(), "", &[]).unwrap_err(),
            "capture requires at least one requested component"
        );
        let registry = registry_with_async_setup(false);
        let ambiguous = select_component(&registry, "").unwrap_err();
        assert!(ambiguous.contains("multiple components present") && ambiguous.contains("--component"));
        let unknown = select_component(&registry, "fixture.absent").unwrap_err();
        assert!(unknown.contains("component 'fixture.absent' not found"));

        let unsupported = discover_capture_target(focused_csharp_binding().to_str().unwrap(), "").unwrap_err();
        assert!(unsupported.contains("only Rust targets") && unsupported.contains("csharp"));
        assert!(matches!(
            capture(focused_rust_binding().to_str().unwrap(), "", "fixture.cli.replay", ""),
            CaptureOutcome::Error { reason } if reason.contains("non-empty output directory")
        ));

        let root = output_dir("profile-preflight");
        std::fs::create_dir_all(&root).unwrap();
        let invalid_profile = root.join("invalid.yaml");
        std::fs::write(&invalid_profile, "version: 2\ninclude: []\n").unwrap();
        let error = capture_profile_result(
            "binding-that-must-not-be-read.yaml",
            "",
            invalid_profile.to_str().unwrap(),
            root.join("capture").to_str().unwrap(),
        )
        .unwrap_err();
        assert!(error.contains("unsupported capture profile version"), "{error}");
        assert!(!root.join("capture").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn compiled_rust_and_csharp_metadata_resolve_the_same_profile() {
        let components = ["fixture.cli.profile_leaf", "fixture.cli.profile_root"];
        let root = output_dir("profile-parity");
        std::fs::create_dir_all(&root).unwrap();
        let profile_path = root.join("profile.yaml");
        std::fs::write(
            &profile_path,
            "version: 1\ninclude:\n  - component: fixture.cli.profile_root\n    operation: root\n  - component: fixture.cli.profile_leaf\n    operation: leaf\n",
        )
        .unwrap();
        let profile = read_capture_profile(&profile_path).unwrap();
        let rust = discover_many_target(focused_rust_binding().to_str().unwrap(), None, &components).unwrap();
        let csharp = discover_many_target(focused_csharp_binding().to_str().unwrap(), None, &components).unwrap();
        let rust_registries = rust.registries.iter().collect::<Vec<_>>();
        let csharp_registries = csharp.registries.iter().collect::<Vec<_>>();
        assert_eq!(
            profile.resolve(&rust_registries).unwrap(),
            profile.resolve(&csharp_registries).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    /// One component whose only setup is async, plus a synchronous sibling
    /// component that shares the operation name.
    fn registry_with_async_setup(setup_is_async: bool) -> Registry {
        let flag = if setup_is_async { "true" } else { "false" };
        let json = format!(
            r#"{{"operations":[
                {{"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.async_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":{flag},"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[["initial","i32"]],"component":"fixture.async_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.sync_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[["initial","i32"]],"component":"fixture.sync_setup"}}
            ],"types":[]}}"#
        );
        Registry::parse(&json).expect("synthetic registry parses")
    }

    /// A single-component selection keeps its historical registry id, while
    /// distinct multi-component profiles must never collide on one id.
    #[test]
    fn selection_registry_ids_are_stable_and_collision_free() {
        assert_eq!(
            selection_registry_id(&BTreeSet::from(["fixture.cli.replay"])),
            "urn:ctsc:registry:fixture.cli.replay"
        );
        let left = selection_registry_id(&BTreeSet::from(["fixture.a", "fixture.b"]));
        let right = selection_registry_id(&BTreeSet::from(["fixture.a", "fixture.c"]));
        assert!(left.starts_with("urn:ctsc:registry:specgate.capture-profile."), "{left}");
        assert_ne!(left, right);
        assert_eq!(left, selection_registry_id(&BTreeSet::from(["fixture.b", "fixture.a"])));
    }

    /// Component mode exports top-level subtrees verbatim: a foreign top-level
    /// operation is dropped whole, and no retained span is ever reparented.
    #[test]
    fn component_filtering_is_top_level_only_and_never_reparents() {
        let capture = synthetic_capture();
        let selected = filter_component_scenarios(std::slice::from_ref(&capture), "fixture.root");
        assert_eq!(
            selected[0]
                .operations
                .iter()
                .map(|operation| (operation.operation_name.as_str(), operation.parent_span_id.as_str()))
                .collect::<Vec<_>>(),
            vec![("root", "scenario"), ("nested", "root-span")]
        );
        assert!(
            filter_component_scenarios(std::slice::from_ref(&capture), "fixture.nested").is_empty(),
            "a component that is never top-level contributes nothing"
        );
        let foreign = filter_component_scenarios(std::slice::from_ref(&capture), "fixture.other");
        assert_eq!(
            foreign[0]
                .operations
                .iter()
                .map(|operation| operation.operation_name.as_str())
                .collect::<Vec<_>>(),
            vec!["other"]
        );
    }

    /// One scenario with two top-level operations from different components,
    /// the first of which has a nested foreign callee.
    fn synthetic_capture() -> NativeCapture {
        let boundary = |span_id: &str, parent: Option<&str>| {
            serde_json::json!({
                "span_id": span_id,
                "parent_span_id": parent,
                "start_time_unix_nano": 0,
                "end_time_unix_nano": 1,
                "status": "ok",
            })
        };
        let operation = |order: u64, name: &str, component: &str, span_id: &str, parent: &str| {
            serde_json::json!({
                "order": order,
                "component_id": component,
                "operation_name": name,
                "span_id": span_id,
                "parent_span_id": parent,
                "start_time_unix_nano": 0,
                "end_time_unix_nano": 1,
                "status": "ok",
                "inputs": {},
                "observations": [],
                "completion": { "kind": "empty", "order": 0, "time_unix_nano": 1 },
            })
        };
        serde_json::from_value(serde_json::json!({
            "trace_id": "11111111111111111111111111111111",
            "scenario_name": "tests::mixed",
            "run": boundary("run", None),
            "scenario": boundary("scenario", Some("run")),
            "operations": [
                operation(0, "root", "fixture.root", "root-span", "scenario"),
                operation(1, "nested", "fixture.nested", "nested-span", "root-span"),
                operation(2, "other", "fixture.other", "other-span", "scenario"),
            ],
        }))
        .expect("synthetic capture deserializes")
    }

    fn request(component: &str) -> CaptureRequest {
        CaptureRequest {
            component: component.to_string(),
            out: PathBuf::from("unused"),
            excluded_operations: BTreeSet::new(),
        }
    }

    /// An async `#[spec_setup]` records no construction inputs, so capturing
    /// its component would encode a bundle that misstates the component's
    /// public input surface. Capture must reject it by name instead.
    #[test]
    fn capture_rejects_a_component_whose_setup_is_async() {
        let registry = registry_with_async_setup(true);
        let reason =
            reject_async_setup_capture(&registry, &[request("fixture.async_setup")]).expect_err("an async setup is not capturable");
        assert!(reason.contains("component 'fixture.async_setup'"), "{reason}");
        assert!(reason.contains("async setup 'make'"), "{reason}");
        assert!(reason.contains("'fixture.async_setup::advance'"), "{reason}");
        assert!(reason.contains("discovery-only"), "{reason}");

        assert!(
            reject_async_setup_capture(&registry, &[request("fixture.sync_setup")]).is_ok(),
            "a synchronous setup on an identically named operation stays capturable"
        );
        assert!(
            reject_async_setup_capture(&registry_with_async_setup(false), &[request("fixture.async_setup")]).is_ok(),
            "the rejection is driven by setup metadata, not by the component name"
        );

        let batched = reject_async_setup_capture(&registry, &[request("fixture.sync_setup"), request("fixture.async_setup")])
            .expect_err("every requested component is screened, not just the first");
        assert!(batched.contains("component 'fixture.async_setup'"), "{batched}");

        let mut excluded = request("fixture.async_setup");
        excluded.excluded_operations.insert("advance".to_string());
        assert!(
            reject_async_setup_capture(&registry, &[excluded]).is_ok(),
            "the golden harness may explicitly exclude the affected operation"
        );
    }

    #[test]
    fn failure_summaries_carry_the_exit_status_and_both_stream_tails() {
        let summary = failure_summary(Some(101), b"running 1 test\ntest add ... FAILED\n", b"  \nstack backtrace: 1\n");
        assert_eq!(
            summary,
            "exit code 101; stdout: running 1 test | test add ... FAILED; stderr: stack backtrace: 1"
        );
        assert_eq!(failure_summary(None, b"", b""), "terminated without an exit code");

        let long = "x".repeat(2_000);
        let truncated = failure_summary(Some(1), long.as_bytes(), b"");
        assert!(truncated.ends_with("..."), "long output is truncated: {truncated}");
        assert!(truncated.chars().count() < 1_300);
    }

    fn request_at(component: &str, out: PathBuf) -> CaptureRequest {
        CaptureRequest {
            component: component.to_string(),
            out,
            excluded_operations: BTreeSet::new(),
        }
    }

    fn directory_file_names(path: &Path) -> Vec<&str> {
        let mut names = std::fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .map(|name| name.to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
            .into_iter()
            .map(|name| match name.as_str() {
                MANIFEST_FILE => MANIFEST_FILE,
                REGISTRY_FILE => REGISTRY_FILE,
                TRACE_FILE => TRACE_FILE,
                other => panic!("unexpected capture artifact: {other}"),
            })
            .collect()
    }
}
