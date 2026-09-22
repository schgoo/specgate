//! `specgate replay <capture-dir> <binding.yaml> --out <candidate.otlp.json>` —
//! statically link CTSC stimuli to a Rust candidate and replay them.

use serde::{Deserialize, Serialize};
use specgate::__rt::NativeCapture;
use specgate::{SpecEvent, spec_operation};
use specgate_ctsc::{
    ReplayBundle, ReplayInput, ReplayType, ReplayValue, decode_replay_bundle_result, encode_replayed_native_captures_otlp_result,
};
use specgate_discovery::binding::resolve_binding_target;
use specgate_discovery::discovery::{DiscoveredOperation, DiscoveredSchema, OpInfo, Registry, discover_many_resolved_target};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const MANIFEST_FILE: &str = "manifest.json";
const REGISTRY_FILE: &str = "registry.ctsc.json";
const REFERENCE_FILE: &str = "reference.otlp.json";
const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe",
    "use", "where", "while", "async", "await", "dyn",
];

static REPLAY_SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

/// Stable categories for replay limitations that a caller may intentionally
/// declare. Errors outside these known limitations remain unclassified and
/// must not be accepted as evidence that replay is unsupported.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayFailureCategory {
    StructuredValue,
    SetupBackedOperation,
    MethodOperation,
    AsyncOperation,
    UnsupportedLanguage,
}

#[cfg(test)]
impl ReplayFailureCategory {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::StructuredValue => "structured-value",
            Self::SetupBackedOperation => "setup-backed-operation",
            Self::MethodOperation => "method-operation",
            Self::AsyncOperation => "async-operation",
            Self::UnsupportedLanguage => "unsupported-language",
        }
    }
}

/// Classify only stable, intentional replay limitations.
///
/// This deliberately does not have a catch-all category: malformed bundles,
/// discovery failures, missing operations, type mismatches, and runner errors
/// are unrelated defects and must fail callers such as the golden gate.
#[cfg(test)]
pub(crate) fn replay_failure_category(reason: &str) -> Option<ReplayFailureCategory> {
    if reason.contains("uses unsupported structured type") || reason.contains("uses unsupported structured replay type") {
        Some(ReplayFailureCategory::StructuredValue)
    } else if reason.contains("is setup-backed; replay does not yet construct setups") {
        Some(ReplayFailureCategory::SetupBackedOperation)
    } else if reason.contains("is a method; replay supports only free functions") {
        Some(ReplayFailureCategory::MethodOperation)
    } else if reason.contains("is async; replay supports only synchronous operations")
        || reason.contains("is async in normalized discovery")
    {
        Some(ReplayFailureCategory::AsyncOperation)
    } else if reason.starts_with("replay currently supports only Rust candidates; binding language is '") {
        Some(ReplayFailureCategory::UnsupportedLanguage)
    } else {
        None
    }
}

/// Summary of a replay run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct ReplayReport {
    #[spec_event]
    pub component_id: String,
    #[spec_event]
    pub scenarios: i32,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub plans: i32,
    #[spec_event]
    pub output_path: String,
}

/// Outcome of `replay`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum ReplayOutcome {
    Complete { report: ReplayReport },
    Error { reason: String },
}

impl std::fmt::Display for ReplayOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayOutcome::Complete { report } => write!(
                f,
                "Complete(component={}, scenarios={}, operations={}, plans={}, output={})",
                report.component_id, report.scenarios, report.operations, report.plans, report.output_path
            ),
            ReplayOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

/// One candidate target described by a serializable replay plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlanTarget {
    pub name: String,
    pub language: String,
    pub package_name: String,
    pub package_version: String,
    pub package_root: String,
    pub runtime: specgate_discovery::support::CargoPackageSource,
}

/// One ordered candidate parameter in a statically linked invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlanInput {
    pub name: String,
    pub semantic_type: ReplayType,
    pub rust_type: String,
}

/// One distinct semantic-to-candidate operation link.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayInvocationLink {
    pub component_id: String,
    pub operation_name: String,
    pub module_path: Vec<String>,
    pub fn_name: String,
    pub inputs: Vec<ReplayPlanInput>,
    pub output: Option<ReplayType>,
}

/// One planned invocation with scenario-specific semantic values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlannedOperation {
    pub link_index: usize,
    pub inputs: Vec<ReplayInput>,
}

/// One ordered candidate replay scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayPlannedScenario {
    pub name: String,
    pub index: i64,
    pub operations: Vec<ReplayPlannedOperation>,
}

/// The complete target-local invocation plan built before code generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayInvocationPlan {
    pub component_id: String,
    pub registry_id: String,
    pub registry_version: String,
    pub registry_digest: String,
    pub target: ReplayPlanTarget,
    pub links: Vec<ReplayInvocationLink>,
    pub scenarios: Vec<ReplayPlannedScenario>,
}

#[derive(Debug)]
struct ResolvedCandidate {
    target_name: String,
    language: String,
    package_name: String,
    package_version: String,
    package_root: PathBuf,
    runtime: specgate_discovery::support::CargoPackageSource,
    raw_registry: Registry,
    schema: DiscoveredSchema,
}

/// Replay every top-level operation in a verified capture bundle against one
/// Rust candidate binding target.
#[must_use]
#[spec_operation("replay")]
pub fn replay(capture_dir: &str, binding: &str, target: &str, out: &str) -> ReplayOutcome {
    match replay_result(capture_dir, binding, target, out) {
        Ok(report) => ReplayOutcome::Complete { report },
        Err(reason) => ReplayOutcome::Error { reason },
    }
}

fn replay_result(capture_dir: &str, binding: &str, target: &str, out: &str) -> Result<ReplayReport, String> {
    if capture_dir.is_empty() {
        return Err("replay requires a non-empty capture directory".to_string());
    }
    if out.is_empty() {
        return Err("replay requires a non-empty output path".to_string());
    }

    let capture_dir = Path::new(capture_dir);
    let manifest = read_bundle_file(capture_dir, MANIFEST_FILE)?;
    let registry = read_bundle_file(capture_dir, REGISTRY_FILE)?;
    let reference = read_bundle_file(capture_dir, REFERENCE_FILE)?;
    let bundle = decode_replay_bundle_result(&manifest, &registry, &reference)?;
    let candidate = discover_candidate(binding, target, &bundle.component_id)?;
    let plan = build_invocation_plan(&bundle, &candidate)?;
    let captures = execute_invocation_plan(&plan)?;
    let mut report = write_replay_output(&plan, &captures, Path::new(out))?;
    report.output_path = out.to_string();
    Ok(report)
}

fn read_bundle_file(capture_dir: &Path, filename: &str) -> Result<Vec<u8>, String> {
    let path = capture_dir.join(filename);
    std::fs::read(&path).map_err(|error| format!("failed to read capture bundle file {}: {error}", path.display()))
}

/// One Rust candidate target discovered once and linked against many capture
/// bundles.
///
/// Candidate discovery is the expensive part of replay, so a batch pays it a
/// single time and every planned component reuses the same link-time metadata.
#[derive(Debug)]
pub(crate) struct ReplayCandidates {
    target_name: String,
    language: String,
    package_name: String,
    package_version: String,
    package_root: PathBuf,
    runtime: specgate_discovery::support::CargoPackageSource,
    raw_registry: Registry,
    schemas: BTreeMap<String, DiscoveredSchema>,
}

impl ReplayCandidates {
    /// Resolve `binding`/`target` and discover every requested component once.
    pub(crate) fn discover(binding: &str, target: &str, components: &[&str]) -> Result<Self, String> {
        let target_name_arg = if target.is_empty() { None } else { Some(target) };
        let resolved = resolve_binding_target(binding, target_name_arg)?;
        let language = resolved.language.clone();
        if language != "rust" {
            return Err(format!(
                "replay currently supports only Rust candidates; binding language is '{language}'"
            ));
        }
        let discovered = discover_many_resolved_target(resolved, components)?;
        let target_name = discovered.target.name.clone();
        let cargo = discovered
            .cargo_context
            .clone()
            .ok_or_else(|| "Rust discovery returned no Cargo source context".to_string())?;
        let package_root = cargo.path;
        if !package_root.join("Cargo.toml").is_file() {
            return Err(format!(
                "candidate target '{target_name}' is not a Rust package (no Cargo.toml at {})",
                package_root.display()
            ));
        }
        let raw_registry = discovered
            .registries
            .first()
            .cloned()
            .ok_or_else(|| "Rust discovery produced no registry document".to_string())?;
        let mut schemas = BTreeMap::new();
        for component in components.iter().filter(|name| !name.is_empty()) {
            let schema = discovered
                .schema(component)
                .ok_or_else(|| format!("candidate discovery returned no metadata for component '{component}'"))?;
            match schema {
                Ok(schema) => {
                    schemas.insert((*component).to_string(), schema.clone());
                }
                Err(reason) => return Err(reason.clone()),
            }
        }
        Ok(Self {
            target_name,
            language,
            package_name: cargo.package,
            package_version: cargo.version,
            package_root,
            runtime: cargo.runtime,
            raw_registry,
            schemas,
        })
    }

    fn component(&self, component: &str) -> Result<ResolvedCandidate, String> {
        let schema = self
            .schemas
            .get(component)
            .ok_or_else(|| format!("candidate was not discovered for component '{component}'"))?;
        Ok(ResolvedCandidate {
            target_name: self.target_name.clone(),
            language: self.language.clone(),
            package_name: self.package_name.clone(),
            package_version: self.package_version.clone(),
            package_root: self.package_root.clone(),
            runtime: self.runtime.clone(),
            raw_registry: self.raw_registry.clone(),
            schema: schema.clone(),
        })
    }

    /// Statically link one verified capture bundle to this candidate without
    /// generating or building a runner.
    #[cfg(test)]
    pub(crate) fn plan(&self, capture_dir: &Path) -> Result<ReplayInvocationPlan, String> {
        let manifest = read_bundle_file(capture_dir, MANIFEST_FILE)?;
        let registry = read_bundle_file(capture_dir, REGISTRY_FILE)?;
        let reference = read_bundle_file(capture_dir, REFERENCE_FILE)?;
        let bundle = decode_replay_bundle_result(&manifest, &registry, &reference)?;
        let candidate = self.component(&bundle.component_id)?;
        build_invocation_plan(&bundle, &candidate)
    }

    /// Execute already-linked plans, reusing one generated runner package and
    /// one Cargo target directory for the whole batch.
    #[cfg(test)]
    pub(crate) fn execute(&self, plans: &[(ReplayInvocationPlan, PathBuf)]) -> Result<Vec<ReplayReport>, String> {
        let scratch = replay_scratch_dir()?;
        let mut reports = Vec::with_capacity(plans.len());
        for (plan, out) in plans {
            if plan.target.package_name != self.package_name {
                return Err(format!(
                    "plan for '{}' targets package '{}', but this candidate is '{}'",
                    plan.component_id, plan.target.package_name, self.package_name
                ));
            }
            let captures = execute_invocation_plan_in(plan, scratch.path())?;
            reports.push(write_replay_output(plan, &captures, out)?);
        }
        Ok(reports)
    }
}

fn write_replay_output(plan: &ReplayInvocationPlan, captures: &[NativeCapture], out: &Path) -> Result<ReplayReport, String> {
    let target_identity = format!("candidate:{}:{}", plan.target.package_name, plan.target.name);
    let encoded = encode_replayed_native_captures_otlp_result(
        captures,
        env!("CARGO_PKG_VERSION"),
        &target_identity,
        &plan.target.language,
        &plan.registry_id,
        &plan.registry_version,
        &plan.registry_digest,
    )?;
    write_output_atomically(out, encoded.otlp_json.as_bytes())?;

    let scenarios = i32::try_from(plan.scenarios.len()).map_err(|_error| "replayed scenario count exceeds i32".to_string())?;
    let operation_count = plan.scenarios.iter().try_fold(0_usize, |count, scenario| {
        count
            .checked_add(scenario.operations.len())
            .ok_or_else(|| "replayed operation count overflow".to_string())
    })?;
    let operations = i32::try_from(operation_count).map_err(|_error| "replayed operation count exceeds i32".to_string())?;
    let plans = i32::try_from(plan.links.len()).map_err(|_error| "replay plan count exceeds i32".to_string())?;
    Ok(ReplayReport {
        component_id: plan.component_id.clone(),
        scenarios,
        operations,
        plans,
        output_path: out.display().to_string(),
    })
}

fn discover_candidate(binding: &str, target: &str, component: &str) -> Result<ResolvedCandidate, String> {
    ReplayCandidates::discover(binding, target, &[component])?.component(component)
}
fn build_invocation_plan(bundle: &ReplayBundle, candidate: &ResolvedCandidate) -> Result<ReplayInvocationPlan, String> {
    let mut links = Vec::new();
    let mut link_indexes = BTreeMap::new();
    let mut scenarios = Vec::new();
    for scenario in &bundle.scenarios {
        let mut operations = Vec::new();
        for operation in &scenario.operations {
            let key = (operation.component_id.clone(), operation.operation_name.clone());
            let link_index = if let Some(index) = link_indexes.get(&key) {
                *index
            } else {
                let declaration = bundle
                    .registry
                    .operations
                    .iter()
                    .find(|declaration| declaration.component_id == operation.component_id && declaration.name == operation.operation_name)
                    .ok_or_else(|| {
                        format!(
                            "reference operation '{}::{}' is absent from its verified registry",
                            operation.component_id, operation.operation_name
                        )
                    })?;
                let link = link_operation(declaration, candidate)?;
                let index = links.len();
                links.push(link);
                link_indexes.insert(key, index);
                index
            };
            operations.push(ReplayPlannedOperation {
                link_index,
                inputs: operation.inputs.clone(),
            });
        }
        scenarios.push(ReplayPlannedScenario {
            name: scenario.name.clone(),
            index: scenario.index,
            operations,
        });
    }
    let plan = ReplayInvocationPlan {
        component_id: bundle.component_id.clone(),
        registry_id: bundle.registry.id.clone(),
        registry_version: bundle.registry.version.clone(),
        registry_digest: bundle.registry.digest.clone(),
        target: ReplayPlanTarget {
            name: candidate.target_name.clone(),
            language: candidate.language.clone(),
            package_name: candidate.package_name.clone(),
            package_version: candidate.package_version.clone(),
            package_root: candidate.package_root.display().to_string(),
            runtime: candidate.runtime.clone(),
        },
        links,
        scenarios,
    };
    serde_json::to_vec(&plan).map_err(|error| format!("failed to serialize target-local invocation plan: {error}"))?;
    Ok(plan)
}

fn link_operation(
    reference: &specgate_ctsc::ReplayRegistryOperation,
    candidate: &ResolvedCandidate,
) -> Result<ReplayInvocationLink, String> {
    let raw_matches = candidate
        .raw_registry
        .ops
        .iter()
        .filter(|operation| !operation.is_setup && operation.component == reference.component_id && operation.name == reference.name)
        .collect::<Vec<_>>();
    let raw = match raw_matches.as_slice() {
        [] => {
            return Err(format!(
                "candidate is missing operation '{}::{}'",
                reference.component_id, reference.name
            ));
        }
        [operation] => *operation,
        operations => {
            return Err(format!(
                "candidate operation '{}::{}' is duplicated {} times",
                reference.component_id,
                reference.name,
                operations.len()
            ));
        }
    };
    if raw.is_async {
        return Err(format!(
            "candidate operation '{}::{}' is async; replay supports only synchronous operations",
            reference.component_id, reference.name
        ));
    }
    if raw.is_method {
        return Err(format!(
            "candidate operation '{}::{}' is a method; replay supports only free functions",
            reference.component_id, reference.name
        ));
    }
    if !raw.is_public {
        return Err(format!(
            "candidate operation '{}::{}' is not public",
            reference.component_id, reference.name
        ));
    }
    if !candidate
        .raw_registry
        .setups_for(&reference.component_id, &reference.name)
        .is_empty()
    {
        return Err(format!(
            "candidate operation '{}::{}' is setup-backed; replay does not yet construct setups",
            reference.component_id, reference.name
        ));
    }

    let normalized_matches = candidate
        .schema
        .operations
        .iter()
        .filter(|operation| operation.name == reference.name)
        .collect::<Vec<_>>();
    let normalized = match normalized_matches.as_slice() {
        [] => {
            return Err(format!(
                "candidate normalized schema is missing operation '{}::{}'",
                reference.component_id, reference.name
            ));
        }
        [operation] => *operation,
        operations => {
            return Err(format!(
                "candidate normalized schema duplicates operation '{}::{}' {} times",
                reference.component_id,
                reference.name,
                operations.len()
            ));
        }
    };
    if normalized.is_async {
        return Err(format!(
            "candidate operation '{}::{}' is async in normalized discovery",
            reference.component_id, reference.name
        ));
    }
    validate_input_surface(reference, normalized)?;
    validate_output_surface(reference, normalized)?;
    if raw.params.len() != normalized.inputs.len()
        || raw
            .params
            .iter()
            .zip(&normalized.inputs)
            .any(|((raw_name, _raw_type), normalized_input)| raw_name != &normalized_input.name)
    {
        return Err(format!(
            "candidate raw metadata parameters for '{}::{}' do not match normalized input names/order",
            reference.component_id, reference.name
        ));
    }

    let module_path = candidate_module_path(raw, &candidate.package_name)?;
    validate_rust_identifier(&raw.fn_name, "candidate function name")?;
    let inputs = reference
        .inputs
        .iter()
        .zip(&raw.params)
        .map(|(input, (_raw_name, rust_type))| {
            validate_supported_rust_input_type(&input.value_type, rust_type, &reference.component_id, &reference.name, &input.name)?;
            Ok(ReplayPlanInput {
                name: input.name.clone(),
                semantic_type: input.value_type.clone(),
                rust_type: rust_type.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(ReplayInvocationLink {
        component_id: reference.component_id.clone(),
        operation_name: reference.name.clone(),
        module_path,
        fn_name: raw.fn_name.clone(),
        inputs,
        output: reference.output.clone(),
    })
}

fn validate_input_surface(reference: &specgate_ctsc::ReplayRegistryOperation, candidate: &DiscoveredOperation) -> Result<(), String> {
    let expected_names = reference.inputs.iter().map(|input| input.name.as_str()).collect::<Vec<_>>();
    let actual_names = candidate.inputs.iter().map(|input| input.name.as_str()).collect::<Vec<_>>();
    if expected_names != actual_names {
        let expected_set = expected_names.iter().copied().collect::<BTreeSet<_>>();
        let actual_set = actual_names.iter().copied().collect::<BTreeSet<_>>();
        let problem = if expected_set == actual_set {
            "input order differs"
        } else {
            "input names differ"
        };
        return Err(format!(
            "candidate operation '{}::{}' {problem}: expected {expected_names:?}, found {actual_names:?}",
            reference.component_id, reference.name
        ));
    }
    for (reference_input, candidate_input) in reference.inputs.iter().zip(&candidate.inputs) {
        let expected_type = supported_semantic_type_name(
            &reference_input.value_type,
            &reference.component_id,
            &reference.name,
            &reference_input.name,
        )?;
        if candidate_input.ty != expected_type {
            return Err(format!(
                "candidate operation '{}::{}' input '{}' type mismatch: expected '{}', found '{}'",
                reference.component_id, reference.name, reference_input.name, expected_type, candidate_input.ty
            ));
        }
    }
    Ok(())
}

fn validate_output_surface(reference: &specgate_ctsc::ReplayRegistryOperation, candidate: &DiscoveredOperation) -> Result<(), String> {
    let expected = match &reference.output {
        Some(value_type) => supported_semantic_type_name(value_type, &reference.component_id, &reference.name, "$result")?,
        None => "",
    };
    if candidate.output != expected {
        return Err(format!(
            "candidate operation '{}::{}' output type mismatch: expected '{}', found '{}'",
            reference.component_id,
            reference.name,
            if expected.is_empty() { "unit" } else { expected },
            if candidate.output.is_empty() { "unit" } else { &candidate.output }
        ));
    }
    if candidate.empty != reference.empty {
        return Err(format!(
            "candidate operation '{}::{}' empty outcome mismatch: expected {}, found {}",
            reference.component_id, reference.name, reference.empty, candidate.empty
        ));
    }
    let expected_errors = reference
        .errors
        .iter()
        .map(|error| {
            let ty = error.value_type.as_ref().map_or_else(
                || Ok(String::new()),
                |value_type| {
                    supported_semantic_type_name(value_type, &reference.component_id, &reference.name, "$error").map(str::to_string)
                },
            )?;
            Ok((error.name.clone(), ty))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let candidate_errors = candidate
        .errors
        .iter()
        .map(|error| (error.name.clone(), error.ty.clone()))
        .collect::<Vec<_>>();
    if candidate_errors != expected_errors {
        return Err(format!(
            "candidate operation '{}::{}' declared errors mismatch: expected {expected_errors:?}, found {candidate_errors:?}",
            reference.component_id, reference.name
        ));
    }
    Ok(())
}

fn supported_semantic_type_name<'a>(
    value_type: &'a ReplayType,
    component: &str,
    operation: &str,
    value_name: &str,
) -> Result<&'a str, String> {
    let Some(name) = value_type.primitive_name() else {
        return Err(format!(
            "operation '{component}::{operation}' value '{value_name}' uses unsupported structured type"
        ));
    };
    match name {
        "unit" | "string" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" => Ok(name),
        other => Err(format!(
            "operation '{component}::{operation}' value '{value_name}' uses unsupported primitive '{other}'"
        )),
    }
}

fn validate_supported_rust_input_type(
    semantic_type: &ReplayType,
    rust_type: &str,
    component: &str,
    operation: &str,
    input: &str,
) -> Result<(), String> {
    let semantic = supported_semantic_type_name(semantic_type, component, operation, input)?;
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    let supported = match semantic {
        "unit" => native == "()",
        "string" => native == "String" || native == "&str",
        "bool" => native == "bool",
        "i32" => native == "i32",
        "i64" => native == "i64",
        "u32" => native == "u32",
        "u64" => native == "u64",
        "f32" => native == "f32",
        "f64" => native == "f64",
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        Err(format!(
            "candidate operation '{component}::{operation}' input '{input}' uses unsupported Rust type '{rust_type}' for semantic type '{semantic}'"
        ))
    }
}

fn candidate_module_path(raw: &OpInfo, package_name: &str) -> Result<Vec<String>, String> {
    let crate_ident = package_name.replace('-', "_");
    let mut segments = raw.module_path.split("::").collect::<Vec<_>>();
    if segments.first().copied() != Some(crate_ident.as_str()) {
        return Err(format!(
            "candidate operation '{}::{}' raw module path '{}' does not begin with crate '{}'",
            raw.component, raw.name, raw.module_path, crate_ident
        ));
    }
    segments.remove(0);
    let mut result = Vec::new();
    for segment in segments {
        validate_rust_identifier(segment, "candidate module path segment")?;
        result.push(segment.to_string());
    }
    Ok(result)
}

fn validate_rust_identifier(value: &str, label: &str) -> Result<(), String> {
    let mut characters = value.chars();
    let valid_start = characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
    if !valid_start || !characters.all(|character| character == '_' || character.is_ascii_alphanumeric()) {
        return Err(format!("{label} '{value}' is not a supported Rust identifier"));
    }
    if RUST_KEYWORDS.contains(&value) {
        return Err(format!(
            "{label} '{value}' requires raw-identifier code generation, which replay does not yet support"
        ));
    }
    Ok(())
}

fn execute_invocation_plan(plan: &ReplayInvocationPlan) -> Result<Vec<NativeCapture>, String> {
    let scratch = replay_scratch_dir()?;
    execute_invocation_plan_in(plan, scratch.path())
}

fn execute_invocation_plan_in(plan: &ReplayInvocationPlan, scratch: &Path) -> Result<Vec<NativeCapture>, String> {
    std::fs::create_dir_all(scratch.join("src"))
        .map_err(|error| format!("failed to scaffold replay runner {}: {error}", scratch.display()))?;
    let plan_path = scratch.join("plan.json");
    let plan_json = serde_json::to_vec(plan).map_err(|error| format!("failed to serialize target-local invocation plan: {error}"))?;
    std::fs::write(&plan_path, plan_json)
        .map_err(|error| format!("failed to write replay invocation plan {}: {error}", plan_path.display()))?;

    let cargo = replay_cargo(plan)?;
    std::fs::write(scratch.join("Cargo.toml"), cargo.manifest)
        .map_err(|error| format!("failed to write replay runner manifest: {error}"))?;
    let registry_config = cargo
        .config
        .map(|config| {
            let path = scratch.join("registry-config.toml");
            std::fs::write(&path, config).map_err(|error| format!("failed to write replay registry config: {error}"))?;
            Ok::<_, String>(path)
        })
        .transpose()?;
    let source = generate_runner_source(plan)?;
    std::fs::write(scratch.join("src").join("main.rs"), source)
        .map_err(|error| format!("failed to write replay runner source: {error}"))?;

    let sidecar = scratch.join("candidate-captures.json");
    let _ = std::fs::remove_file(&sidecar);
    let mut command = Command::new(cargo_bin());
    command.arg("run").arg("--quiet");
    if let Some(config) = registry_config {
        command.arg("--config").arg(config);
    }
    command
        .arg("--manifest-path")
        .arg(scratch.join("Cargo.toml"))
        .arg("--")
        .arg(&sidecar);
    command.current_dir(&plan.target.package_root);
    command.env_remove("RUSTC_WORKSPACE_WRAPPER");
    command.env_remove("CARGO");
    command.env_remove("CARGO_MANIFEST_DIR");
    command.env("CARGO_TARGET_DIR", scratch.join("target"));
    let output = command
        .output()
        .map_err(|error| format!("failed to run candidate replay runner: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "candidate replay runner failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let captures_json =
        std::fs::read(&sidecar).map_err(|error| format!("candidate replay runner did not write native capture sidecar: {error}"))?;
    let captures: Vec<NativeCapture> =
        serde_json::from_slice(&captures_json).map_err(|error| format!("candidate replay native capture sidecar is malformed: {error}"))?;
    if captures.len() != plan.scenarios.len() {
        return Err(format!(
            "candidate replay runner produced {} captures for {} planned scenarios",
            captures.len(),
            plan.scenarios.len()
        ));
    }
    Ok(captures)
}

fn replay_scratch_dir() -> Result<specgate_discovery::support::InvocationCache, String> {
    let id = REPLAY_SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    specgate_discovery::support::InvocationCache::create("replay", "candidate", id)
}

fn replay_cargo(plan: &ReplayInvocationPlan) -> Result<specgate_discovery::support::RunnerCargo, String> {
    let dependencies = BTreeMap::from([
        (
            "candidate".to_string(),
            specgate_discovery::support::ManifestDependency::local(
                plan.target.package_name.clone(),
                plan.target.package_version.clone(),
                PathBuf::from(&plan.target.package_root),
            ),
        ),
        (
            "serde_json".to_string(),
            specgate_discovery::support::ManifestDependency {
                package: "serde_json".to_string(),
                version: Some("1".to_string()),
                path: None,
                registry: None,
            },
        ),
        (
            "specgate_runtime".to_string(),
            specgate_discovery::support::ManifestDependency::from_source(&plan.target.runtime),
        ),
    ]);
    specgate_discovery::support::runner_cargo("specgate-replay-runner", dependencies)
}

fn generate_runner_source(plan: &ReplayInvocationPlan) -> Result<String, String> {
    let mut source = String::from(
        "fn main() -> Result<(), String> {\n    let sidecar = std::env::args_os().nth(1).ok_or_else(|| \"missing capture sidecar path\".to_string())?;\n    let mut captures = Vec::new();\n",
    );
    for (scenario_position, scenario) in plan.scenarios.iter().enumerate() {
        let identity = u64::try_from(scenario_position)
            .map_err(|_error| "replay scenario identity exceeds u64".to_string())?
            .checked_add(1)
            .ok_or_else(|| "replay scenario identity overflow".to_string())?;
        let trace_id = format!("2{identity:031x}");
        let run_span_id = format!("2{identity:013x}01");
        let scenario_span_id = format!("2{identity:013x}02");
        let start_time = i64::try_from(scenario_position)
            .map_err(|_error| "replay scenario timestamp exceeds i64".to_string())?
            .checked_mul(1_000_000)
            .and_then(|value| value.checked_add(10_000_000))
            .ok_or_else(|| "replay scenario timestamp overflow".to_string())?;
        source.push_str("    specgate_runtime::start_native_capture(specgate_runtime::NativeCaptureConfig {\n");
        write!(
            source,
            "        scenario_name: {}.to_string(),\n        trace_id: {}.to_string(),\n        run_span_id: {}.to_string(),\n        scenario_span_id: {}.to_string(),\n        operation_span_ids: Vec::new(),\n        start_time_unix_nano: {start_time},\n        clock_step_unix_nano: 1,\n    }})?;\n",
            rust_string_literal(&scenario.name)?,
            rust_string_literal(&trace_id)?,
            rust_string_literal(&run_span_id)?,
            rust_string_literal(&scenario_span_id)?,
        )
        .expect("writing to a String cannot fail");
        for operation in &scenario.operations {
            let link = plan
                .links
                .get(operation.link_index)
                .ok_or_else(|| format!("planned operation references missing link {}", operation.link_index))?;
            if operation.inputs.len() != link.inputs.len() {
                return Err(format!(
                    "planned operation '{}::{}' has {} values for {} inputs",
                    link.component_id,
                    link.operation_name,
                    operation.inputs.len(),
                    link.inputs.len()
                ));
            }
            let mut path = String::from("candidate");
            for segment in &link.module_path {
                path.push_str("::");
                path.push_str(segment);
            }
            path.push_str("::");
            path.push_str(&link.fn_name);
            let arguments = operation
                .inputs
                .iter()
                .zip(&link.inputs)
                .map(|(input, definition)| {
                    if input.name != definition.name || input.value_type != definition.semantic_type {
                        return Err(format!(
                            "planned input '{}' does not match link input '{}'",
                            input.name, definition.name
                        ));
                    }
                    render_replay_value(&input.value, &definition.rust_type)
                })
                .collect::<Result<Vec<_>, String>>()?
                .join(", ");
            writeln!(source, "    let _ = {path}({arguments});").expect("writing to a String cannot fail");
        }
        source.push_str("    captures.push(specgate_runtime::finish_native_capture()?);\n");
    }
    source.push_str(
        "    let json = serde_json::to_vec(&captures).map_err(|error| format!(\"failed to serialize native captures: {error}\"))?;\n    std::fs::write(sidecar, json).map_err(|error| format!(\"failed to write native capture sidecar: {error}\"))?;\n    Ok(())\n}\n",
    );
    Ok(source)
}

fn render_replay_value(value: &ReplayValue, rust_type: &str) -> Result<String, String> {
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    match (value, native.as_str()) {
        (ReplayValue::Unit, "()") => Ok("()".to_string()),
        (ReplayValue::String(value), "String") => Ok(format!("String::from({})", rust_string_literal(value)?)),
        (ReplayValue::String(value), "&str") => rust_string_literal(value),
        (ReplayValue::Bool(value), "bool") => Ok(value.to_string()),
        (ReplayValue::I32(value), "i32") => Ok(format!("{value}_i32")),
        (ReplayValue::I64(value), "i64") => Ok(format!("{value}_i64")),
        (ReplayValue::U32(value), "u32") => Ok(format!("{value}_u32")),
        (ReplayValue::U64(value), "u64") => Ok(format!("{value}_u64")),
        (ReplayValue::F32Bits(bits), "f32") => Ok(format!("f32::from_bits({bits}_u32)")),
        (ReplayValue::F64Bits(bits), "f64") => Ok(format!("f64::from_bits({bits}_u64)")),
        _ => Err(format!(
            "replay value {value:?} cannot be passed losslessly to candidate Rust type '{rust_type}'"
        )),
    }
}

fn rust_string_literal(value: &str) -> Result<String, String> {
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('"');
    for character in value.chars() {
        match character {
            '\0' => literal.push_str("\\0"),
            '\u{8}' => literal.push_str("\\x08"),
            '\u{c}' => literal.push_str("\\x0c"),
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\r' => literal.push_str("\\r"),
            '\n' => literal.push_str("\\n"),
            '\t' => literal.push_str("\\t"),
            character if character.is_control() => {
                write!(literal, "\\u{{{:x}}}", u32::from(character))
                    .map_err(|error| format!("failed to encode Rust string literal: {error}"))?;
            }
            character => literal.push(character),
        }
    }
    literal.push('"');
    Ok(literal)
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

fn write_output_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("failed to create replay output directory {}: {error}", parent.display()))?;
    let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or("candidate.otlp.json");
    let temporary = parent.join(format!(".{filename}.specgate-replay-{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&temporary)
            .map_err(|error| format!("failed to create replay temporary output {}: {error}", temporary.display()))?;
        file.write_all(bytes)
            .map_err(|error| format!("failed to write replay temporary output {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("failed to sync replay temporary output {}: {error}", temporary.display()))?;
        if path.exists() {
            std::fs::remove_file(path).map_err(|error| format!("failed to replace existing replay output {}: {error}", path.display()))?;
        }
        std::fs::rename(&temporary, path).map_err(|error| format!("failed to publish replay output {}: {error}", path.display()))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Format a replay outcome for CLI display.
#[must_use]
pub fn format_outcome(outcome: &ReplayOutcome) -> String {
    format!("{outcome}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureOutcome, capture};
    use specgate_ctsc::{
        ReplayOperation, ReplayRegistry, ReplayRegistryOperation, ReplayScenario,
        validation::{validate_linked, validate_trace},
    };
    use specgate_discovery::discovery::DiscoveredInput;

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
            .expect("repository root")
            .to_path_buf()
    }

    fn rust_binding() -> PathBuf {
        repo_root().join("test").join("bindings").join("rust.yaml")
    }

    fn output_dir(label: &str) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-replay-test-{label}-{}", std::process::id()))
    }

    fn cargo_path(path: &Path) -> String {
        let display = path.display().to_string();
        display.strip_prefix(r"\\?\").unwrap_or(&display).replace('\\', "/")
    }

    fn configured_candidate() -> (specgate_discovery::support::InvocationCache, PathBuf) {
        let cache = specgate_discovery::support::InvocationCache::create(
            "tests",
            "candidate-cargo-config",
            REPLAY_SCRATCH_ID.fetch_add(1, Ordering::Relaxed),
        )
        .unwrap();
        let candidate = cache.path().join("candidate");
        let proof = cache.path().join("specgate-config-proof");
        std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::create_dir_all(proof.join("src")).unwrap();
        std::fs::write(
            proof.join("Cargo.toml"),
            "[package]\nname=\"specgate-config-proof\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(proof.join("src").join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"configured-candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n\
                 [dependencies]\nspecgate={{path=\"{}\"}}\nspecgate-config-proof=\"0.1.0\"\n[workspace]\n",
                cargo_path(&repo_root().join("rust").join("crates").join("specgate"))
            ),
        )
        .unwrap();
        std::fs::write(
            candidate.join("src").join("lib.rs"),
            "#![allow(unexpected_cfgs)]\n\
             #[cfg(not(specgate_candidate_config))]\n\
             compile_error!(\"candidate .cargo/config.toml was not loaded\");\n\
             use specgate::{spec_component, spec_operation};\n\
             spec_component!(\"fixture.configured\");\n\
             #[spec_operation(\"add\")]\n\
             pub fn add(a: i32, b: i32) -> i32 { specgate_config_proof::add(a, b) }\n\
             #[test]\n\
             fn adds_two_and_three() { assert_eq!(add(2, 3), 5); }\n",
        )
        .unwrap();
        std::fs::write(
            candidate.join(".cargo").join("config.toml"),
            format!(
                "[build]\nrustflags=[\"--cfg\", \"specgate_candidate_config\"]\n\
                 [patch.crates-io]\nspecgate-config-proof={{path=\"{}\"}}\n\
                 [net]\noffline=true\n",
                cargo_path(&proof)
            ),
        )
        .unwrap();
        let binding = cache.path().join("binding.yaml");
        std::fs::write(
            &binding,
            format!(
                "language: rust\ntargets:\n  default:\n    package_root: {}\n",
                cargo_path(&candidate)
            ),
        )
        .unwrap();
        (cache, binding)
    }

    #[test]
    fn replay_stateless_capture_is_linked_independent_and_byte_identical() {
        let capture_dir = output_dir("capture");
        let first_output = output_dir("first").with_extension("otlp.json");
        let second_output = output_dir("second").with_extension("otlp.json");
        let _ = std::fs::remove_dir_all(&capture_dir);
        let _ = std::fs::remove_file(&first_output);
        let _ = std::fs::remove_file(&second_output);

        let captured = capture(
            rust_binding().to_str().unwrap(),
            "",
            "fixture.stateless_add",
            capture_dir.to_str().unwrap(),
        );
        assert!(matches!(captured, CaptureOutcome::Complete { .. }), "capture failed: {captured}");
        let first = replay(
            capture_dir.to_str().unwrap(),
            rust_binding().to_str().unwrap(),
            "",
            first_output.to_str().unwrap(),
        );
        let second = replay(
            capture_dir.to_str().unwrap(),
            rust_binding().to_str().unwrap(),
            "",
            second_output.to_str().unwrap(),
        );
        let ReplayOutcome::Complete { report } = first else {
            panic!("first replay failed: {first}");
        };
        assert!(matches!(second, ReplayOutcome::Complete { .. }), "second replay failed: {second}");
        assert_eq!(
            report,
            ReplayReport {
                component_id: "fixture.stateless_add".to_string(),
                scenarios: 1,
                operations: 1,
                plans: 1,
                output_path: first_output.display().to_string(),
            }
        );
        let first_bytes = std::fs::read(&first_output).unwrap();
        assert_eq!(first_bytes, std::fs::read(&second_output).unwrap());

        let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(capture_dir.join(REFERENCE_FILE)).unwrap()).unwrap();
        let candidate: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        let reference_trace = &reference["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"];
        let candidate_trace = &candidate["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"];
        assert_ne!(reference_trace, candidate_trace);
        let candidate_text = String::from_utf8(first_bytes).unwrap();
        assert!(candidate_text.contains("\"conformance.target.name\""));
        assert!(candidate_text.contains("candidate:specgate-ctsc-fixtures:default"));
        assert!(candidate_text.contains("\"conformance.operation.name\""));
        assert!(candidate_text.contains("\"add\""));
        assert!(candidate_text.contains("\"a\""));
        assert!(candidate_text.contains("\"intValue\":\"2\""));
        assert!(candidate_text.contains("\"b\""));
        assert!(candidate_text.contains("\"intValue\":\"3\""));
        assert!(candidate_text.contains("\"conformance.result\""));
        assert!(candidate_text.contains("\"intValue\":\"5\""));
        let trace_validation = validate_trace(&first_output);
        assert!(
            trace_validation.valid,
            "native trace validation failed: {:#?}",
            trace_validation.issues
        );
        let linked_validation = validate_linked(&first_output, &capture_dir.join(REGISTRY_FILE), &[]);
        assert!(
            linked_validation.valid,
            "native linked validation failed: {:#?}",
            linked_validation.issues
        );

        let csharp_binding = repo_root().join("test").join("bindings").join("csharp.yaml");
        let unsupported_output = output_dir("unsupported-language").with_extension("otlp.json");
        assert!(matches!(
            replay(
                capture_dir.to_str().unwrap(),
                csharp_binding.to_str().unwrap(),
                "",
                unsupported_output.to_str().unwrap()
            ),
            ReplayOutcome::Error { reason } if reason.contains("only Rust candidates") && reason.contains("csharp")
        ));
        assert!(!unsupported_output.exists());

        let _ = std::fs::remove_dir_all(capture_dir);
        let _ = std::fs::remove_file(first_output);
        let _ = std::fs::remove_file(second_output);
    }

    #[test]
    fn candidate_cargo_config_applies_to_metadata_discovery_and_replay() {
        let (cache, binding) = configured_candidate();
        let capture_dir = cache.path().join("capture");
        let replay_output = cache.path().join("candidate.otlp.json");
        let captured = capture(binding.to_str().unwrap(), "", "fixture.configured", capture_dir.to_str().unwrap());
        assert!(matches!(captured, CaptureOutcome::Complete { .. }), "capture failed: {captured}");
        let replayed = replay(
            capture_dir.to_str().unwrap(),
            binding.to_str().unwrap(),
            "",
            replay_output.to_str().unwrap(),
        );
        assert!(matches!(replayed, ReplayOutcome::Complete { .. }), "replay failed: {replayed}");
        assert!(replay_output.is_file());
    }

    #[test]
    fn replay_preserves_rust_native_string_escaping() {
        let capture_dir = output_dir("string-capture");
        let candidate_output = output_dir("string-candidate").with_extension("otlp.json");
        let _ = std::fs::remove_dir_all(&capture_dir);
        let _ = std::fs::remove_file(&candidate_output);

        let captured = capture(
            rust_binding().to_str().unwrap(),
            "",
            "fixture.strings",
            capture_dir.to_str().unwrap(),
        );
        assert!(matches!(captured, CaptureOutcome::Complete { .. }), "capture failed: {captured}");
        let replayed = replay(
            capture_dir.to_str().unwrap(),
            rust_binding().to_str().unwrap(),
            "",
            candidate_output.to_str().unwrap(),
        );
        assert!(matches!(replayed, ReplayOutcome::Complete { .. }), "replay failed: {replayed}");

        let document: serde_json::Value = serde_json::from_slice(&std::fs::read(&candidate_output).unwrap()).unwrap();
        let operation = document["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|span| span["name"] == "conformance.operation")
            .unwrap();
        let inputs = operation["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attribute| attribute["key"] == "conformance.operation.inputs")
            .unwrap();
        let value = inputs["value"]["kvlistValue"]["values"][0]["value"]["stringValue"]
            .as_str()
            .unwrap();
        assert_eq!(
            value,
            "nul:\0 backspace:\u{8} formfeed:\u{c} quote:\" slash:\\ cr:\r lf:\n tab:\t unicode:雪🙂"
        );

        let _ = std::fs::remove_dir_all(capture_dir);
        let _ = std::fs::remove_file(candidate_output);
    }

    #[test]
    fn planner_rejects_semantic_and_target_shape_mismatches() {
        let base = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        assert_plan_error(&candidate_without_operation(&base), "missing operation");

        let mut duplicate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        duplicate.raw_registry.ops.push(raw_operation("add"));
        assert_plan_error(&duplicate, "duplicated");

        let mut renamed = normalized_operation("add");
        renamed.inputs[0].name = "left".to_string();
        assert_plan_error(&candidate_metadata(raw_operation("add"), renamed), "input names differ");

        let mut reordered = normalized_operation("add");
        reordered.inputs.swap(0, 1);
        assert_plan_error(&candidate_metadata(raw_operation("add"), reordered), "input order differs");

        let mut input_type = normalized_operation("add");
        input_type.inputs[0].ty = "i64".to_string();
        assert_plan_error(&candidate_metadata(raw_operation("add"), input_type), "input 'a' type mismatch");

        let mut output_type = normalized_operation("add");
        output_type.output = "i64".to_string();
        assert_plan_error(&candidate_metadata(raw_operation("add"), output_type), "output type mismatch");

        let mut asynchronous = raw_operation("add");
        asynchronous.is_async = true;
        assert_plan_error(&candidate_metadata(asynchronous, normalized_operation("add")), "is async");

        let mut method = raw_operation("add");
        method.is_method = true;
        assert_plan_error(&candidate_metadata(method, normalized_operation("add")), "is a method");

        let mut private = raw_operation("add");
        private.is_public = false;
        assert_plan_error(&candidate_metadata(private, normalized_operation("add")), "not public");

        let mut setup = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let mut setup_operation = raw_operation("add");
        setup_operation.is_setup = true;
        setup.raw_registry.ops.push(setup_operation);
        assert_plan_error(&setup, "setup-backed");
    }

    #[test]
    fn planner_rejects_structured_values_and_unsupported_language() {
        let mut bundle = reference_bundle();
        bundle.registry.operations[0].inputs[0].value_type = ReplayType::List {
            items: Box::new(ReplayType::Primitive { name: "i32".to_string() }),
        };
        bundle.scenarios[0].operations[0].inputs[0].value_type = bundle.registry.operations[0].inputs[0].value_type.clone();
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let reason = build_invocation_plan(&bundle, &candidate).unwrap_err();
        assert!(reason.contains("unsupported structured type"));
        assert_eq!(replay_failure_category(&reason), Some(ReplayFailureCategory::StructuredValue));
    }

    #[test]
    fn replay_failure_categories_are_stable_and_do_not_classify_unrelated_errors() {
        for (reason, expected) in [
            (
                "candidate operation 'fixture.counter::increment' is setup-backed; replay does not yet construct setups",
                ReplayFailureCategory::SetupBackedOperation,
            ),
            (
                "candidate operation 'fixture.counter::increment' is a method; replay supports only free functions",
                ReplayFailureCategory::MethodOperation,
            ),
            (
                "candidate operation 'fixture.fetch::fetch' is async; replay supports only synchronous operations",
                ReplayFailureCategory::AsyncOperation,
            ),
            (
                "replay currently supports only Rust candidates; binding language is 'csharp'",
                ReplayFailureCategory::UnsupportedLanguage,
            ),
        ] {
            assert_eq!(replay_failure_category(reason), Some(expected), "{reason}");
        }
        assert_eq!(ReplayFailureCategory::StructuredValue.code(), "structured-value");
        assert_eq!(
            replay_failure_category("operation span '3' result uses unsupported structured replay type named"),
            Some(ReplayFailureCategory::StructuredValue)
        );
        assert_eq!(replay_failure_category("candidate is missing operation 'fixture.add::add'"), None);
        assert_eq!(replay_failure_category("capture bundle digest mismatch"), None);
    }

    #[test]
    fn packaged_replay_manifest_uses_candidate_runtime_dependency() {
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let plan = build_invocation_plan(&reference_bundle(), &candidate).unwrap();
        let cargo = replay_cargo(&plan).unwrap();
        let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
        assert_eq!(parsed["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
        assert_eq!(
            parsed["dependencies"]["specgate_runtime"]["path"].as_str(),
            Some("resolved-runtime")
        );
        assert_eq!(cargo.config, None);
    }

    fn reference_bundle() -> ReplayBundle {
        let i32_type = ReplayType::Primitive { name: "i32".to_string() };
        let inputs = vec![
            ReplayInput {
                name: "a".to_string(),
                value_type: i32_type.clone(),
                value: ReplayValue::I32(2),
            },
            ReplayInput {
                name: "b".to_string(),
                value_type: i32_type.clone(),
                value: ReplayValue::I32(3),
            },
        ];
        ReplayBundle {
            component_id: "fixture.stateless_add".to_string(),
            registry: ReplayRegistry {
                id: "urn:ctsc:registry:fixture.stateless_add".to_string(),
                version: "0.1.0".to_string(),
                digest: format!("sha256:{}", "a".repeat(64)),
                operations: vec![ReplayRegistryOperation {
                    component_id: "fixture.stateless_add".to_string(),
                    name: "add".to_string(),
                    inputs: inputs
                        .iter()
                        .map(|input| specgate_ctsc::ReplayRegistryInput {
                            name: input.name.clone(),
                            value_type: input.value_type.clone(),
                        })
                        .collect(),
                    output: Some(i32_type),
                    empty: false,
                    errors: Vec::new(),
                }],
            },
            scenarios: vec![ReplayScenario {
                name: "add".to_string(),
                index: 0,
                operations: vec![ReplayOperation {
                    component_id: "fixture.stateless_add".to_string(),
                    operation_name: "add".to_string(),
                    inputs,
                    output: Some(ReplayType::Primitive { name: "i32".to_string() }),
                }],
            }],
        }
    }

    fn raw_operation(name: &str) -> OpInfo {
        OpInfo {
            name: name.to_string(),
            module_path: "specgate_ctsc_fixtures::stateless".to_string(),
            fn_name: name.to_string(),
            is_setup: false,
            is_async: false,
            is_method: false,
            is_public: true,
            return_type: "i32".to_string(),
            fills: String::new(),
            params: vec![("a".to_string(), "i32".to_string()), ("b".to_string(), "i32".to_string())],
            component: "fixture.stateless_add".to_string(),
            cs_class: None,
            cs_method_of: None,
            cs_method: None,
            cs_is_static: None,
            cs_return: None,
            cs_params: Vec::new(),
            cs_exceptions: None,
        }
    }

    fn normalized_operation(name: &str) -> DiscoveredOperation {
        DiscoveredOperation {
            name: name.to_string(),
            is_async: false,
            inputs: vec![
                DiscoveredInput {
                    name: "a".to_string(),
                    ty: "i32".to_string(),
                },
                DiscoveredInput {
                    name: "b".to_string(),
                    ty: "i32".to_string(),
                },
            ],
            output: "i32".to_string(),
            empty: false,
            errors: Vec::new(),
            setups: Vec::new(),
        }
    }

    fn candidate_metadata(raw: OpInfo, normalized: DiscoveredOperation) -> ResolvedCandidate {
        ResolvedCandidate {
            target_name: "default".to_string(),
            language: "rust".to_string(),
            package_name: "specgate-ctsc-fixtures".to_string(),
            package_version: "0.1.0".to_string(),
            package_root: PathBuf::from("candidate"),
            runtime: specgate_discovery::support::CargoPackageSource {
                package: "specgate-runtime".to_string(),
                version: "0.6.0".to_string(),
                path: Some(PathBuf::from("resolved-runtime")),
                registry: None,
            },
            raw_registry: Registry {
                ops: vec![raw],
                types: Vec::new(),
            },
            schema: DiscoveredSchema {
                component: "fixture.stateless_add".to_string(),
                dependencies: Vec::new(),
                dependency_types: Vec::new(),
                operations: vec![normalized],
                types: Vec::new(),
            },
        }
    }

    fn candidate_without_operation(base: &ResolvedCandidate) -> ResolvedCandidate {
        ResolvedCandidate {
            target_name: base.target_name.clone(),
            language: base.language.clone(),
            package_name: base.package_name.clone(),
            package_version: base.package_version.clone(),
            package_root: base.package_root.clone(),
            runtime: base.runtime.clone(),
            raw_registry: Registry {
                ops: Vec::new(),
                types: Vec::new(),
            },
            schema: DiscoveredSchema {
                component: base.schema.component.clone(),
                dependencies: Vec::new(),
                dependency_types: Vec::new(),
                operations: Vec::new(),
                types: Vec::new(),
            },
        }
    }

    fn assert_plan_error(candidate: &ResolvedCandidate, expected: &str) {
        let error = build_invocation_plan(&reference_bundle(), candidate).unwrap_err();
        assert!(error.contains(expected), "expected '{expected}' in '{error}'");
    }
}
