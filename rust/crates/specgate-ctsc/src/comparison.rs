//! Deterministic CTSC Strict differential comparison.

use crate::validation::{
    AnyValue, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceEvent, TraceSpan, TypeRef, ValidationIssue,
    canonical_typed_value, find_operation, load_registry_set, load_trace, validate_linked_model,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

/// CTSC Strict policy identifier.
pub const STRICT_POLICY: &str = "ctsc.strict";
/// CTSC Strict policy version.
pub const STRICT_POLICY_VERSION: &str = "0.1.0";

/// One deterministic behavioral mismatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonMismatch {
    /// Semantic location.
    pub path: String,
    /// Reference semantic value.
    pub expected: String,
    /// Candidate semantic value.
    pub actual: String,
}

/// Unsupported or ambiguous input under CTSC Strict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonError {
    /// Semantic location.
    pub path: String,
    /// Failure description.
    pub message: String,
}

/// Typed result of one CTSC Strict comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// Policy identifier.
    pub policy: String,
    /// Policy version.
    pub policy_version: String,
    /// Reference artifact path.
    pub reference: String,
    /// Candidate artifact path.
    pub candidate: String,
    /// True only when validation succeeds and no differences exist.
    pub equivalent: bool,
    /// Trace Core, Registry, or Linked validation failures.
    pub validation_failures: Vec<ValidationIssue>,
    /// Strict-policy unsupported or ambiguous conditions.
    pub errors: Vec<ComparisonError>,
    /// Ordered semantic differences.
    pub mismatches: Vec<ComparisonMismatch>,
}

/// Compare two CTSC traces with fixed `ctsc.strict/0.1.0` semantics.
#[must_use]
pub fn compare(reference: &Path, candidate: &Path, registry: Option<&Path>, imports: &[PathBuf]) -> ComparisonReport {
    let reference_trace = load_trace(reference);
    let candidate_trace = load_trace(candidate);
    let registry_set = registry.map(|path| load_registry_set(path, imports));
    let mut validation_failures = Vec::new();
    validation_failures.extend(prefix_issues("reference", reference_trace.issues));
    validation_failures.extend(prefix_issues("candidate", candidate_trace.issues));
    if let Some(registry_set) = &registry_set {
        validation_failures.extend(prefix_issues("registry", registry_set.issues.clone()));
    }
    if validation_failures.is_empty()
        && let (Some(reference_trace), Some(candidate_trace)) = (reference_trace.value.as_ref(), candidate_trace.value.as_ref())
        && let Some(registry_set) = registry_set.as_ref().and_then(|loaded| loaded.value.as_ref())
    {
        let mut linked = Vec::new();
        validate_linked_model(reference_trace, registry_set, &mut linked);
        validation_failures.extend(prefix_issues("reference linked", linked));
        let mut linked = Vec::new();
        validate_linked_model(candidate_trace, registry_set, &mut linked);
        validation_failures.extend(prefix_issues("candidate linked", linked));
    }

    let mut errors = Vec::new();
    let mut mismatches = Vec::new();
    if validation_failures.is_empty()
        && let (Some(reference_trace), Some(candidate_trace)) = (reference_trace.value.as_ref(), candidate_trace.value.as_ref())
    {
        let registry = registry_set.as_ref().and_then(|loaded| loaded.value.as_ref());
        compare_documents(reference_trace, candidate_trace, registry, &mut errors, &mut mismatches);
    }
    ComparisonReport {
        policy: STRICT_POLICY.to_string(),
        policy_version: STRICT_POLICY_VERSION.to_string(),
        reference: reference.display().to_string(),
        candidate: candidate.display().to_string(),
        equivalent: validation_failures.is_empty() && errors.is_empty() && mismatches.is_empty(),
        validation_failures,
        errors,
        mismatches,
    }
}

fn prefix_issues(prefix: &str, issues: Vec<ValidationIssue>) -> Vec<ValidationIssue> {
    issues
        .into_iter()
        .map(|issue| ValidationIssue {
            location: format!("{prefix}:{}", issue.location),
            message: issue.message,
        })
        .collect()
}

struct TraceIndex<'a> {
    children: BTreeMap<(&'a str, &'a str), Vec<&'a TraceSpan>>,
}

impl<'a> TraceIndex<'a> {
    fn new(document: &'a TraceDocument) -> Self {
        let mut children = BTreeMap::<(&str, &str), Vec<&TraceSpan>>::new();
        for span in &document.spans {
            children
                .entry((span.trace_id.as_str(), span.parent_span_id.as_str()))
                .or_default()
                .push(span);
        }
        Self { children }
    }

    fn children(&self, span: &TraceSpan) -> Vec<&'a TraceSpan> {
        self.children
            .get(&(span.trace_id.as_str(), span.span_id.as_str()))
            .cloned()
            .unwrap_or_default()
    }
}

fn compare_documents(
    reference: &TraceDocument,
    candidate: &TraceDocument,
    registry: Option<&RegistrySet>,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_runs = reference
        .spans
        .iter()
        .filter(|span| span.name == "conformance.run")
        .collect::<Vec<_>>();
    let candidate_runs = candidate
        .spans
        .iter()
        .filter(|span| span.name == "conformance.run")
        .collect::<Vec<_>>();
    if reference_runs.len() != 1 {
        comparison_error(
            errors,
            "reference.run",
            &format!("implicit run selection requires exactly one run, found {}", reference_runs.len()),
        );
    }
    if candidate_runs.len() != 1 {
        comparison_error(
            errors,
            "candidate.run",
            &format!("implicit run selection requires exactly one run, found {}", candidate_runs.len()),
        );
    }
    let (Some(reference_run), Some(candidate_run)) = (reference_runs.first(), candidate_runs.first()) else {
        return;
    };
    if !errors.is_empty() {
        return;
    }
    let reference_index = TraceIndex::new(reference);
    let candidate_index = TraceIndex::new(candidate);
    compare_events(
        &reference_run.events,
        &candidate_run.events,
        None,
        None,
        registry,
        "run",
        mismatches,
    );

    let reference_scenarios = scenarios(reference_run, &reference_index, "reference", errors);
    let candidate_scenarios = scenarios(candidate_run, &candidate_index, "candidate", errors);
    let names = reference_scenarios
        .keys()
        .chain(candidate_scenarios.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in names {
        let path = format!("scenario[{name:?}]");
        match (reference_scenarios.get(&name), candidate_scenarios.get(&name)) {
            (Some(reference), Some(candidate)) => compare_container(
                reference,
                candidate,
                &reference_index,
                &candidate_index,
                registry,
                &path,
                errors,
                mismatches,
            ),
            (Some(_), None) => mismatch(mismatches, &path, "present", "<missing>"),
            (None, Some(_)) => mismatch(mismatches, &path, "<missing>", "present"),
            (None, None) => {}
        }
    }
}

fn scenarios<'a>(
    run: &'a TraceSpan,
    index: &TraceIndex<'a>,
    side: &str,
    errors: &mut Vec<ComparisonError>,
) -> BTreeMap<String, &'a TraceSpan> {
    let mut result = BTreeMap::new();
    for scenario in index.children(run) {
        if scenario.name != "conformance.scenario" {
            continue;
        }
        let name = scenario
            .attributes
            .get("conformance.scenario.name")
            .and_then(AnyValue::as_string)
            .unwrap_or_default()
            .to_string();
        if result.insert(name.clone(), scenario).is_some() {
            comparison_error(
                errors,
                &format!("{side}.scenario[{name:?}]"),
                "scenario names must be unique within a run",
            );
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn compare_container(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    compare_events(&reference.events, &candidate.events, None, None, registry, path, mismatches);
    compare_children(
        reference,
        candidate,
        reference_index,
        candidate_index,
        registry,
        path,
        errors,
        mismatches,
    );
}

#[allow(clippy::too_many_arguments)]
fn compare_children(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    if reference.name == "conformance.parallel" || candidate.name == "conformance.parallel" {
        compare_parallel_children(
            reference,
            candidate,
            reference_index,
            candidate_index,
            registry,
            path,
            errors,
            mismatches,
        );
        return;
    }
    let Some(reference_children) = sequential_children(reference, reference_index, "reference", path, errors) else {
        return;
    };
    let Some(candidate_children) = sequential_children(candidate, candidate_index, "candidate", path, errors) else {
        return;
    };
    let count = reference_children.len().max(candidate_children.len());
    for position in 0..count {
        let child_path = format!("{path}.children[{position}]");
        match (reference_children.get(position), candidate_children.get(position)) {
            (Some(reference), Some(candidate)) => compare_child(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                &child_path,
                errors,
                mismatches,
            ),
            (Some(reference), None) => mismatch(mismatches, &child_path, &span_identity(reference).to_string(), "<missing>"),
            (None, Some(candidate)) => mismatch(mismatches, &child_path, "<missing>", &span_identity(candidate).to_string()),
            (None, None) => {}
        }
    }
}

fn sequential_children<'a>(
    parent: &TraceSpan,
    index: &TraceIndex<'a>,
    side: &str,
    path: &str,
    errors: &mut Vec<ComparisonError>,
) -> Option<Vec<&'a TraceSpan>> {
    let mut children = index.children(parent);
    if children
        .iter()
        .any(|span| span.start_time.zip(span.end_time).is_none_or(|(start, end)| end < start))
    {
        comparison_error(
            errors,
            &format!("{side}.{path}.children"),
            "child spans require valid start/end intervals for strict ordering",
        );
        return None;
    }
    for left in 0..children.len() {
        for right in left + 1..children.len() {
            let left_interval = children[left].start_time.zip(children[left].end_time).expect("validated interval");
            let right_interval = children[right]
                .start_time
                .zip(children[right].end_time)
                .expect("validated interval");
            let left_before_right = left_interval.1 <= right_interval.0;
            let right_before_left = right_interval.1 <= left_interval.0;
            if left_before_right && right_before_left {
                comparison_error(
                    errors,
                    &format!("{side}.{path}.children"),
                    "equal child intervals make strict sequential ordering ambiguous",
                );
                return None;
            }
            if !left_before_right && !right_before_left {
                comparison_error(
                    errors,
                    &format!("{side}.{path}.children"),
                    "overlapping child spans outside conformance.parallel are unsupported",
                );
                return None;
            }
        }
    }
    children.sort_by_key(|span| (span.start_time, span.end_time));
    Some(children)
}

#[allow(clippy::too_many_arguments)]
fn compare_parallel_children(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_children = identity_map(reference_index.children(reference), "reference", path, errors);
    let candidate_children = identity_map(candidate_index.children(candidate), "candidate", path, errors);
    let (Some(reference_children), Some(candidate_children)) = (reference_children, candidate_children) else {
        return;
    };
    let identities = reference_children
        .keys()
        .chain(candidate_children.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for identity in identities {
        let child_path = format!("{path}.parallel[{identity}]");
        match (reference_children.get(&identity), candidate_children.get(&identity)) {
            (Some(reference), Some(candidate)) => compare_child(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                &child_path,
                errors,
                mismatches,
            ),
            (Some(_), None) => mismatch(mismatches, &child_path, "present", "<missing>"),
            (None, Some(_)) => mismatch(mismatches, &child_path, "<missing>", "present"),
            (None, None) => {}
        }
    }
}

fn identity_map<'a>(
    children: Vec<&'a TraceSpan>,
    side: &str,
    path: &str,
    errors: &mut Vec<ComparisonError>,
) -> Option<BTreeMap<SpanIdentity<'a>, &'a TraceSpan>> {
    let mut result = BTreeMap::new();
    let mut ambiguous = false;
    for child in children {
        let identity = span_identity(child);
        if result.insert(identity, child).is_some() {
            comparison_error(
                errors,
                &format!("{side}.{path}.parallel[{identity}]"),
                "duplicate parallel branch semantic identity is ambiguous",
            );
            ambiguous = true;
        }
    }
    (!ambiguous).then_some(result)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SpanIdentity<'a> {
    Operation { component: &'a str, name: &'a str },
    Span(&'a str),
}

impl fmt::Display for SpanIdentity<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation { component, name } => {
                write!(formatter, "operation[component={component:?},name={name:?}]")
            }
            Self::Span(name) => formatter.write_str(name),
        }
    }
}

fn span_identity(span: &TraceSpan) -> SpanIdentity<'_> {
    if span.name == "conformance.operation" {
        let component = span
            .attributes
            .get("conformance.component.id")
            .and_then(AnyValue::as_string)
            .unwrap_or("<missing>");
        let operation = span
            .attributes
            .get("conformance.operation.name")
            .and_then(AnyValue::as_string)
            .unwrap_or("<missing>");
        SpanIdentity::Operation {
            component,
            name: operation,
        }
    } else {
        SpanIdentity::Span(&span.name)
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_child(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    if reference.name != candidate.name {
        mismatch(mismatches, &format!("{path}.span"), &reference.name, &candidate.name);
        return;
    }
    match reference.name.as_str() {
        "conformance.operation" => {
            let operation_path = format!(
                "{path}.operation[{}::{}]",
                attribute_string(reference, "conformance.component.id"),
                attribute_string(reference, "conformance.operation.name")
            );
            compare_operation(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                &operation_path,
                errors,
                mismatches,
            );
        }
        "conformance.parallel" => {
            compare_events(&reference.events, &candidate.events, None, None, registry, path, mismatches);
            compare_parallel_children(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                path,
                errors,
                mismatches,
            );
        }
        _ => mismatch(mismatches, &format!("{path}.span"), &reference.name, &candidate.name),
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_operation(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonError>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_component = attribute_string(reference, "conformance.component.id");
    let candidate_component = attribute_string(candidate, "conformance.component.id");
    let reference_name = attribute_string(reference, "conformance.operation.name");
    let candidate_name = attribute_string(candidate, "conformance.operation.name");
    if reference_component != candidate_component {
        mismatch(mismatches, &format!("{path}.component"), reference_component, candidate_component);
    }
    if reference_name != candidate_name {
        mismatch(mismatches, &format!("{path}.operation"), reference_name, candidate_name);
    }
    let identities_match = reference_component == candidate_component && reference_name == candidate_name;
    let declaration = identities_match
        .then(|| registry.and_then(|registry| find_operation(registry, reference_component, reference_name)))
        .flatten();
    compare_inputs(reference, candidate, declaration, registry, path, mismatches);
    compare_events(
        &reference.events,
        &candidate.events,
        declaration,
        Some(reference_component),
        registry,
        path,
        mismatches,
    );
    compare_children(
        reference,
        candidate,
        reference_index,
        candidate_index,
        registry,
        path,
        errors,
        mismatches,
    );
}

fn compare_inputs(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    declaration: Option<(&ResolvedComponent, &RegistryOperation)>,
    registry: Option<&RegistrySet>,
    path: &str,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_inputs = reference
        .attributes
        .get("conformance.operation.inputs")
        .and_then(AnyValue::as_kvlist)
        .expect("validated operation inputs");
    let candidate_inputs = candidate
        .attributes
        .get("conformance.operation.inputs")
        .and_then(AnyValue::as_kvlist)
        .expect("validated operation inputs");
    let names = reference_inputs
        .keys()
        .chain(candidate_inputs.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for name in names {
        let value_type = declaration.and_then(|(_, operation)| {
            operation
                .inputs
                .iter()
                .find(|input| input.name == name)
                .map(|input| &input.value_type)
        });
        compare_value(
            reference_inputs.get(&name),
            candidate_inputs.get(&name),
            value_type,
            declaration.map(|(component, _)| component),
            registry,
            &format!("{path}.inputs.{name}"),
            mismatches,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_events(
    reference: &[TraceEvent],
    candidate: &[TraceEvent],
    declaration: Option<(&ResolvedComponent, &RegistryOperation)>,
    _component_id: Option<&str>,
    registry: Option<&RegistrySet>,
    path: &str,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let count = reference.len().max(candidate.len());
    for position in 0..count {
        let event_path = format!("{path}.events[{position}]");
        let (Some(reference), Some(candidate)) = (reference.get(position), candidate.get(position)) else {
            match (reference.get(position), candidate.get(position)) {
                (Some(reference), None) => mismatch(mismatches, &event_path, &reference.name, "<missing>"),
                (None, Some(candidate)) => mismatch(mismatches, &event_path, "<missing>", &candidate.name),
                (None, None) => {}
                (Some(_), Some(_)) => unreachable!(),
            }
            continue;
        };
        if reference.name != candidate.name {
            mismatch(mismatches, &format!("{event_path}.name"), &reference.name, &candidate.name);
            continue;
        }
        match reference.name.as_str() {
            "conformance.observation" => {
                let reference_name = event_attribute_string(reference, "conformance.observation.name");
                let candidate_name = event_attribute_string(candidate, "conformance.observation.name");
                if reference_name != candidate_name {
                    mismatch(mismatches, &format!("{event_path}.observation"), reference_name, candidate_name);
                }
                let value_type = (reference_name == candidate_name)
                    .then(|| {
                        declaration.and_then(|(_, operation)| {
                            operation
                                .observations
                                .iter()
                                .find(|observation| observation.name == reference_name)
                                .map(|observation| &observation.value_type)
                        })
                    })
                    .flatten();
                compare_value(
                    reference.attributes.get("conformance.observation.value"),
                    candidate.attributes.get("conformance.observation.value"),
                    value_type,
                    declaration.map(|(component, _)| component),
                    registry,
                    &format!("{event_path}.value"),
                    mismatches,
                );
            }
            "conformance.result" => compare_value(
                reference.attributes.get("conformance.result.value"),
                candidate.attributes.get("conformance.result.value"),
                declaration.and_then(|(_, operation)| operation.outcomes.result.as_ref()),
                declaration.map(|(component, _)| component),
                registry,
                &format!("{event_path}.result"),
                mismatches,
            ),
            "conformance.error" => {
                let reference_name = event_attribute_string(reference, "conformance.error.name");
                let candidate_name = event_attribute_string(candidate, "conformance.error.name");
                if reference_name != candidate_name {
                    mismatch(mismatches, &format!("{event_path}.error"), reference_name, candidate_name);
                }
                let value_type = (reference_name == candidate_name)
                    .then(|| {
                        declaration.and_then(|(_, operation)| {
                            operation
                                .outcomes
                                .errors
                                .iter()
                                .find(|error| error.name == reference_name)
                                .and_then(|error| error.value_type.as_ref())
                        })
                    })
                    .flatten();
                compare_value(
                    reference.attributes.get("conformance.error.value"),
                    candidate.attributes.get("conformance.error.value"),
                    value_type,
                    declaration.map(|(component, _)| component),
                    registry,
                    &format!("{event_path}.value"),
                    mismatches,
                );
            }
            "conformance.fault" => {
                for key in ["conformance.fault.type", "conformance.fault.message"] {
                    compare_value(
                        reference.attributes.get(key),
                        candidate.attributes.get(key),
                        None,
                        None,
                        None,
                        &format!("{event_path}.{key}"),
                        mismatches,
                    );
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_value(
    reference: Option<&AnyValue>,
    candidate: Option<&AnyValue>,
    value_type: Option<&TypeRef>,
    component: Option<&ResolvedComponent>,
    registry: Option<&RegistrySet>,
    path: &str,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    match (reference, candidate) {
        (Some(reference), Some(candidate)) => {
            if let (Some(value_type), Some(component), Some(registry)) = (value_type, component, registry) {
                let reference = canonical_typed_value(reference, value_type, component, registry);
                let candidate = canonical_typed_value(candidate, value_type, component, registry);
                if reference != candidate {
                    mismatch(
                        mismatches,
                        path,
                        &reference.map_or_else(|| "<invalid>".to_string(), |value| value.display()),
                        &candidate.map_or_else(|| "<invalid>".to_string(), |value| value.display()),
                    );
                }
            } else if reference != candidate {
                mismatch(mismatches, path, &reference.display(), &candidate.display());
            }
        }
        (Some(reference), None) => mismatch(mismatches, path, &reference.display(), "<missing>"),
        (None, Some(candidate)) => mismatch(mismatches, path, "<missing>", &candidate.display()),
        (None, None) => {}
    }
}

fn attribute_string<'a>(span: &'a TraceSpan, key: &str) -> &'a str {
    span.attributes.get(key).and_then(AnyValue::as_string).unwrap_or("<missing>")
}

fn event_attribute_string<'a>(event: &'a TraceEvent, key: &str) -> &'a str {
    event.attributes.get(key).and_then(AnyValue::as_string).unwrap_or("<missing>")
}

fn mismatch(mismatches: &mut Vec<ComparisonMismatch>, path: &str, expected: &str, actual: &str) {
    mismatches.push(ComparisonMismatch {
        path: path.to_string(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    });
}

fn comparison_error(errors: &mut Vec<ComparisonError>, path: &str, message: &str) {
    errors.push(ComparisonError {
        path: path.to_string(),
        message: message.to_string(),
    });
}
