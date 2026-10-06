//! Private CTSC Strict indexing and semantic comparison engine.

use super::{
    ComparisonDiagnostic, ComparisonMismatch, ComparisonReport, DocumentReader, POLICY_ID, POLICY_VERSION, SemanticValue, SystemReader,
    ValidationDiagnostic,
};
use crate::validation::{
    AnyValue, DocumentBytes, Loaded, RegistryOperation, RegistrySet, ResolvedComponent, TraceDocument, TraceEvent, TraceSpan, TypeRef,
    ValidationIssue, canonical_value, check_linked, find_operation, load_bytes, registry,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::path::{Path, PathBuf};

// CTSC Trace Core names used as semantic comparison keys.
const RUN_SPAN: &str = "conformance.run";
const SCENARIO_SPAN: &str = "conformance.scenario";
const OPERATION_SPAN: &str = "conformance.operation";
const PARALLEL_SPAN: &str = "conformance.parallel";
const SCENARIO_ATTR: &str = "conformance.scenario.name";
const COMPONENT_ATTR: &str = "conformance.component.id";
const OP_NAME_ATTR: &str = "conformance.operation.name";
const OP_INPUTS_ATTR: &str = "conformance.operation.inputs";
const OBS_EVENT: &str = "conformance.observation";
const OBS_NAME_ATTR: &str = "conformance.observation.name";
const OBS_VALUE_ATTR: &str = "conformance.observation.value";
const RESULT: &str = "conformance.result";
const RESULT_ATTR: &str = "conformance.result.value";
const ERROR: &str = "conformance.error";
const ERROR_NAME_ATTR: &str = "conformance.error.name";
const ERROR_VALUE_ATTR: &str = "conformance.error.value";
const FAULT: &str = "conformance.fault";
const FAULT_TYPE_ATTR: &str = "conformance.fault.type";
const FAULT_MESSAGE_ATTR: &str = "conformance.fault.message";
const MISSING: &str = "<missing>";
const INVALID: &str = "<invalid>";
const PRESENT: &str = "present";

pub(crate) fn compare_paths(
    reference: impl AsRef<Path>,
    candidate: impl AsRef<Path>,
    registry: Option<impl AsRef<Path>>,
    imports: impl AsRef<[PathBuf]>,
) -> ComparisonReport {
    compare_with(reference, candidate, registry, imports, &SystemReader::system())
}

/// Compare path-named documents through an injected reader.
///
/// # Examples
///
/// ```no_run
/// use specgate_ctsc::comparison::{compare_with, SystemReader};
///
/// let report = compare_with(
///     "reference.otlp.json",
///     "candidate.otlp.json",
///     Some("registry.ctsc.json"),
///     Vec::<std::path::PathBuf>::new(),
///     &SystemReader::system(),
/// );
/// assert!(report.equivalent || !report.validation_failures.is_empty());
/// ```
#[must_use]
#[expect(clippy::needless_pass_by_value, reason = "path-like values support owned and borrowed callers")]
pub fn compare_with(
    reference: impl AsRef<Path>,
    candidate: impl AsRef<Path>,
    registry: Option<impl AsRef<Path>>,
    imports: impl AsRef<[PathBuf]>,
    reader: &impl DocumentReader,
) -> ComparisonReport {
    let reference = reference.as_ref();
    let candidate = candidate.as_ref();
    let reference_trace = crate::validation::trace::load_from(reference, reader);
    let candidate_trace = crate::validation::trace::load_from(candidate, reader);
    let registry_set = registry
        .as_ref()
        .map(|path| registry::load_reader(path.as_ref(), imports.as_ref(), reader));
    compare_loaded(reference, candidate, reference_trace, candidate_trace, registry_set.as_ref())
}

/// Compare named in-memory CTSC bytes with the same behavior as [`crate::compare`].
///
/// # Examples
///
/// ```
/// use specgate_ctsc::{compare_documents, validation::DocumentBytes};
/// use std::path::Path;
/// let artifact = DocumentBytes::new(Path::new("trace.otlp.json"), br#"{"resourceSpans":[]}"#);
/// let report = compare_documents(artifact, artifact, None, &[]);
/// assert!(!report.validation_failures.is_empty());
/// ```
#[must_use]
pub(crate) fn compare_bytes(
    reference: DocumentBytes<'_>,
    candidate: DocumentBytes<'_>,
    registry: Option<DocumentBytes<'_>>,
    imports: &[DocumentBytes<'_>],
) -> ComparisonReport {
    let reference_trace = load_bytes(reference.path, reference.bytes);
    let candidate_trace = load_bytes(candidate.path, candidate.bytes);
    let registry_set = registry.map(|root| registry::load_bytes(root, imports));
    compare_loaded(
        reference.path,
        candidate.path,
        reference_trace,
        candidate_trace,
        registry_set.as_ref(),
    )
}

fn compare_loaded(
    reference: &Path,
    candidate: &Path,
    reference_trace: Loaded<TraceDocument>,
    candidate_trace: Loaded<TraceDocument>,
    registry_set: Option<&Loaded<RegistrySet>>,
) -> ComparisonReport {
    let validation_capacity =
        reference_trace.issues.len() + candidate_trace.issues.len() + registry_set.map_or(0, |loaded| loaded.issues.len());
    let mut validation_failures = Vec::with_capacity(validation_capacity);
    validation_failures.extend(prefix_issues("reference", reference_trace.issues));
    validation_failures.extend(prefix_issues("candidate", candidate_trace.issues));
    if let Some(registry_set) = registry_set {
        validation_failures.extend(prefix_issues("registry", registry_set.issues.clone()));
    }
    if validation_failures.is_empty()
        && let (Some(reference_trace), Some(candidate_trace)) = (reference_trace.value.as_ref(), candidate_trace.value.as_ref())
        && let Some(registry_set) = registry_set.and_then(|loaded| loaded.value.as_ref())
    {
        let mut linked = Vec::new();
        check_linked(reference_trace, registry_set, &mut linked);
        validation_failures.extend(prefix_issues("reference linked", linked));
        let mut linked = Vec::new();
        check_linked(candidate_trace, registry_set, &mut linked);
        validation_failures.extend(prefix_issues("candidate linked", linked));
    }

    let mut errors = Vec::new();
    let mut mismatches = Vec::new();
    if validation_failures.is_empty()
        && let (Some(reference_trace), Some(candidate_trace)) = (reference_trace.value.as_ref(), candidate_trace.value.as_ref())
    {
        let registry = registry_set.and_then(|loaded| loaded.value.as_ref());
        compare_documents(reference_trace, candidate_trace, registry, &mut errors, &mut mismatches);
    }
    validation_failures.shrink_to_fit();
    errors.shrink_to_fit();
    mismatches.shrink_to_fit();
    ComparisonReport {
        policy: POLICY_ID.to_string(),
        policy_version: POLICY_VERSION.to_string(),
        reference: reference.to_path_buf(),
        candidate: candidate.to_path_buf(),
        equivalent: validation_failures.is_empty() && errors.is_empty() && mismatches.is_empty(),
        validation_failures,
        errors,
        mismatches,
    }
}

fn prefix_issues(prefix: &str, issues: Vec<ValidationIssue>) -> Vec<ValidationDiagnostic> {
    issues
        .into_iter()
        .map(|issue| ValidationDiagnostic {
            location: format!("{prefix}:{}", issue.location).into(),
            message: issue.message.into(),
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
        for spans in children.values_mut() {
            spans.shrink_to_fit();
        }
        Self { children }
    }

    fn children<'b>(&'b self, span: &'b TraceSpan) -> &'b [&'a TraceSpan] {
        self.children
            .get(&(span.trace_id.as_str(), span.span_id.as_str()))
            .map_or(&[], Vec::as_slice)
    }
}

fn compare_documents(
    reference: &TraceDocument,
    candidate: &TraceDocument,
    registry: Option<&RegistrySet>,
    errors: &mut Vec<ComparisonDiagnostic>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_runs = reference.spans.iter().filter(|span| span.name == RUN_SPAN).collect::<Vec<_>>();
    let candidate_runs = candidate.spans.iter().filter(|span| span.name == RUN_SPAN).collect::<Vec<_>>();
    if reference_runs.len() != 1 {
        comparison_error(
            "reference.run",
            &format!("implicit run selection requires exactly one run, found {}", reference_runs.len()),
            errors,
        );
    }
    if candidate_runs.len() != 1 {
        comparison_error(
            "candidate.run",
            &format!("implicit run selection requires exactly one run, found {}", candidate_runs.len()),
            errors,
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
    let mut path = String::new();
    for name in names {
        path.clear();
        write!(path, "scenario[{name:?}]").expect("writing to a String cannot fail");
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
            (Some(_), None) => mismatch(&path, PRESENT, MISSING, mismatches),
            (None, Some(_)) => mismatch(&path, MISSING, PRESENT, mismatches),
            (None, None) => {}
        }
    }
}

fn scenarios<'a>(
    run: &'a TraceSpan,
    index: &TraceIndex<'a>,
    side: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
) -> BTreeMap<String, &'a TraceSpan> {
    let mut result = BTreeMap::new();
    for &scenario in index.children(run) {
        if scenario.name != SCENARIO_SPAN {
            continue;
        }
        let name = scenario
            .attributes
            .get(SCENARIO_ATTR)
            .and_then(AnyValue::as_string)
            .unwrap_or_default()
            .to_string();
        if result.insert(name.clone(), scenario).is_some() {
            comparison_error(
                &format!("{side}.scenario[{name:?}]"),
                "scenario names must be unique within a run",
                errors,
            );
        }
    }
    result
}

#[expect(
    clippy::too_many_arguments,
    reason = "comparison recursion carries both trace indexes and both diagnostic sinks"
)]
fn compare_container(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
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

#[expect(
    clippy::too_many_arguments,
    reason = "comparison recursion carries both trace indexes and both diagnostic sinks"
)]
fn compare_children(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    if reference.name == PARALLEL_SPAN || candidate.name == PARALLEL_SPAN {
        compare_parallel(
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
    let mut child_path = String::with_capacity(path.len() + ".children[]".len() + usize::MAX.ilog10() as usize + 1);
    for position in 0..count {
        child_path.clear();
        write!(child_path, "{path}.children[{position}]").expect("writing to a String cannot fail");
        match (reference_children.get(position), candidate_children.get(position)) {
            (Some(reference), Some(candidate)) => compare_child(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                &mut child_path,
                errors,
                mismatches,
            ),
            (Some(reference), None) => mismatch(&child_path, span_identity(reference).to_string(), MISSING, mismatches),
            (None, Some(candidate)) => mismatch(&child_path, MISSING, span_identity(candidate).to_string(), mismatches),
            (None, None) => {}
        }
    }
}

fn sequential_children<'a>(
    parent: &TraceSpan,
    index: &TraceIndex<'a>,
    side: &str,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
) -> Option<Vec<&'a TraceSpan>> {
    let mut children = index.children(parent).to_vec();
    if children
        .iter()
        .any(|span| span.start_time.zip(span.end_time).is_none_or(|(start, end)| end < start))
    {
        comparison_error(
            &format!("{side}.{path}.children"),
            "child spans require valid start/end intervals for strict ordering",
            errors,
        );
        return None;
    }
    children.sort_by_key(|span| (span.start_time, span.end_time));
    for adjacent in children.windows(2) {
        let left = adjacent[0].start_time.zip(adjacent[0].end_time).expect("validated interval");
        let right = adjacent[1].start_time.zip(adjacent[1].end_time).expect("validated interval");
        if left.1 <= right.0 && right.1 <= left.0 {
            comparison_error(
                &format!("{side}.{path}.children"),
                "equal child intervals make strict sequential ordering ambiguous",
                errors,
            );
            return None;
        }
        if left.1 > right.0 {
            comparison_error(
                &format!("{side}.{path}.children"),
                "overlapping child spans outside conformance.parallel are unsupported",
                errors,
            );
            return None;
        }
    }
    Some(children)
}

#[expect(
    clippy::too_many_arguments,
    reason = "parallel comparison carries both trace indexes and both diagnostic sinks"
)]
fn compare_parallel(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
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
    let mut child_path = String::with_capacity(path.len() + ".parallel[]".len());
    for identity in identities {
        child_path.clear();
        write!(child_path, "{path}.parallel[{identity}]").expect("writing to a String cannot fail");
        match (reference_children.get(&identity), candidate_children.get(&identity)) {
            (Some(reference), Some(candidate)) => compare_child(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                &mut child_path,
                errors,
                mismatches,
            ),
            (Some(_), None) => mismatch(&child_path, PRESENT, MISSING, mismatches),
            (None, Some(_)) => mismatch(&child_path, MISSING, PRESENT, mismatches),
            (None, None) => {}
        }
    }
}

fn identity_map<'a>(
    children: &[&'a TraceSpan],
    side: &str,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
) -> Option<BTreeMap<SpanIdentity<'a>, &'a TraceSpan>> {
    let mut result = BTreeMap::new();
    let mut ambiguous = false;
    for &child in children {
        let identity = span_identity(child);
        if result.insert(identity, child).is_some() {
            comparison_error(
                &format!("{side}.{path}.parallel[{identity}]"),
                "duplicate parallel branch semantic identity is ambiguous",
                errors,
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
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation { component, name } => {
                write!(f, "operation[component={component:?},name={name:?}]")
            }
            Self::Span(name) => f.write_str(name),
        }
    }
}

fn span_identity(span: &TraceSpan) -> SpanIdentity<'_> {
    if span.name == OPERATION_SPAN {
        let component = span.attributes.get(COMPONENT_ATTR).and_then(AnyValue::as_string).unwrap_or(MISSING);
        let operation = span.attributes.get(OP_NAME_ATTR).and_then(AnyValue::as_string).unwrap_or(MISSING);
        SpanIdentity::Operation {
            component,
            name: operation,
        }
    } else {
        SpanIdentity::Span(&span.name)
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "child comparison carries both trace indexes and both diagnostic sinks"
)]
fn compare_child(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &mut String,
    errors: &mut Vec<ComparisonDiagnostic>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    if reference.name != candidate.name {
        let base_length = path.len();
        path.push_str(".span");
        mismatch(path, &reference.name, &candidate.name, mismatches);
        path.truncate(base_length);
        return;
    }
    match reference.name.as_str() {
        OPERATION_SPAN => {
            let base_length = path.len();
            write!(
                path,
                ".operation[{}::{}]",
                attribute_string(reference, COMPONENT_ATTR),
                attribute_string(reference, OP_NAME_ATTR)
            )
            .expect("writing to a String cannot fail");
            compare_operation(
                reference,
                candidate,
                reference_index,
                candidate_index,
                registry,
                path,
                errors,
                mismatches,
            );
            path.truncate(base_length);
        }
        PARALLEL_SPAN => {
            compare_events(&reference.events, &candidate.events, None, None, registry, path, mismatches);
            compare_parallel(
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
        _ => {
            let base_length = path.len();
            path.push_str(".span");
            mismatch(path, &reference.name, &candidate.name, mismatches);
            path.truncate(base_length);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "operation comparison carries both trace indexes and both diagnostic sinks"
)]
fn compare_operation(
    reference: &TraceSpan,
    candidate: &TraceSpan,
    reference_index: &TraceIndex<'_>,
    candidate_index: &TraceIndex<'_>,
    registry: Option<&RegistrySet>,
    path: &str,
    errors: &mut Vec<ComparisonDiagnostic>,
    mismatches: &mut Vec<ComparisonMismatch>,
) {
    let reference_component = attribute_string(reference, COMPONENT_ATTR);
    let candidate_component = attribute_string(candidate, COMPONENT_ATTR);
    let reference_name = attribute_string(reference, OP_NAME_ATTR);
    let candidate_name = attribute_string(candidate, OP_NAME_ATTR);
    if reference_component != candidate_component {
        mismatch(&format!("{path}.component"), reference_component, candidate_component, mismatches);
    }
    if reference_name != candidate_name {
        mismatch(&format!("{path}.operation"), reference_name, candidate_name, mismatches);
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
        .get(OP_INPUTS_ATTR)
        .and_then(AnyValue::as_kvlist)
        .expect("validated operation inputs");
    let candidate_inputs = candidate
        .attributes
        .get(OP_INPUTS_ATTR)
        .and_then(AnyValue::as_kvlist)
        .expect("validated operation inputs");
    let names = reference_inputs
        .keys()
        .chain(candidate_inputs.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let max_name_len = names.iter().map(String::len).max().unwrap_or_default();
    let mut input_path = String::with_capacity(path.len() + ".inputs.".len() + max_name_len);
    for name in names {
        let value_type = declaration.and_then(|(_, operation)| {
            operation
                .inputs
                .iter()
                .find(|input| input.name == name)
                .map(|input| &input.value_type)
        });
        input_path.clear();
        write!(input_path, "{path}.inputs.{name}").expect("writing to a String cannot fail");
        compare_value(
            reference_inputs.get(&name),
            candidate_inputs.get(&name),
            value_type,
            declaration.map(|(component, _)| component),
            registry,
            &input_path,
            mismatches,
        );
    }
}

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
    let mut event_path = String::with_capacity(path.len() + ".events[]".len() + usize::MAX.ilog10() as usize + 1);
    let mut detail_path = String::with_capacity(event_path.capacity() + FAULT_MESSAGE_ATTR.len() + 1);
    for position in 0..count {
        event_path.clear();
        write!(event_path, "{path}.events[{position}]").expect("writing to a String cannot fail");
        let (Some(reference), Some(candidate)) = (reference.get(position), candidate.get(position)) else {
            match (reference.get(position), candidate.get(position)) {
                (Some(reference), None) => mismatch(&event_path, &reference.name, MISSING, mismatches),
                (None, Some(candidate)) => mismatch(&event_path, MISSING, &candidate.name, mismatches),
                (None, None) => {}
                (Some(_), Some(_)) => unreachable!("the preceding let-else rejected the case where both events exist"),
            }
            continue;
        };
        if reference.name != candidate.name {
            detail_path.clear();
            write!(detail_path, "{event_path}.name").expect("writing to a String cannot fail");
            mismatch(&detail_path, &reference.name, &candidate.name, mismatches);
            continue;
        }
        match reference.name.as_str() {
            OBS_EVENT => {
                let reference_name = event_string(reference, OBS_NAME_ATTR);
                let candidate_name = event_string(candidate, OBS_NAME_ATTR);
                if reference_name != candidate_name {
                    detail_path.clear();
                    write!(detail_path, "{event_path}.observation").expect("writing to a String cannot fail");
                    mismatch(&detail_path, reference_name, candidate_name, mismatches);
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
                detail_path.clear();
                write!(detail_path, "{event_path}.value").expect("writing to a String cannot fail");
                compare_value(
                    reference.attributes.get(OBS_VALUE_ATTR),
                    candidate.attributes.get(OBS_VALUE_ATTR),
                    value_type,
                    declaration.map(|(component, _)| component),
                    registry,
                    &detail_path,
                    mismatches,
                );
            }
            RESULT => {
                detail_path.clear();
                write!(detail_path, "{event_path}.result").expect("writing to a String cannot fail");
                compare_value(
                    reference.attributes.get(RESULT_ATTR),
                    candidate.attributes.get(RESULT_ATTR),
                    declaration.and_then(|(_, operation)| operation.outcomes.result.as_ref()),
                    declaration.map(|(component, _)| component),
                    registry,
                    &detail_path,
                    mismatches,
                );
            }
            ERROR => {
                let reference_name = event_string(reference, ERROR_NAME_ATTR);
                let candidate_name = event_string(candidate, ERROR_NAME_ATTR);
                if reference_name != candidate_name {
                    detail_path.clear();
                    write!(detail_path, "{event_path}.error").expect("writing to a String cannot fail");
                    mismatch(&detail_path, reference_name, candidate_name, mismatches);
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
                detail_path.clear();
                write!(detail_path, "{event_path}.value").expect("writing to a String cannot fail");
                compare_value(
                    reference.attributes.get(ERROR_VALUE_ATTR),
                    candidate.attributes.get(ERROR_VALUE_ATTR),
                    value_type,
                    declaration.map(|(component, _)| component),
                    registry,
                    &detail_path,
                    mismatches,
                );
            }
            FAULT => {
                for key in [FAULT_TYPE_ATTR, FAULT_MESSAGE_ATTR] {
                    detail_path.clear();
                    write!(detail_path, "{event_path}.{key}").expect("writing to a String cannot fail");
                    compare_value(
                        reference.attributes.get(key),
                        candidate.attributes.get(key),
                        None,
                        None,
                        None,
                        &detail_path,
                        mismatches,
                    );
                }
            }
            _ => {}
        }
    }
}

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
                let reference = canonical_value(reference, value_type, component, registry);
                let candidate = canonical_value(candidate, value_type, component, registry);
                if reference != candidate {
                    mismatch(
                        path,
                        reference.map_or_else(|| INVALID.to_string(), |value| value.display()),
                        candidate.map_or_else(|| INVALID.to_string(), |value| value.display()),
                        mismatches,
                    );
                }
            } else if reference != candidate {
                mismatch(path, reference.display(), candidate.display(), mismatches);
            }
        }
        (Some(reference), None) => mismatch(path, reference.display(), MISSING, mismatches),
        (None, Some(candidate)) => mismatch(path, MISSING, candidate.display(), mismatches),
        (None, None) => {}
    }
}

fn attribute_string<'a>(span: &'a TraceSpan, key: &str) -> &'a str {
    span.attributes.get(key).and_then(AnyValue::as_string).unwrap_or(MISSING)
}

fn event_string<'a>(event: &'a TraceEvent, key: &str) -> &'a str {
    event.attributes.get(key).and_then(AnyValue::as_string).unwrap_or(MISSING)
}

fn mismatch(path: &str, expected: impl Into<String>, actual: impl Into<String>, mismatches: &mut Vec<ComparisonMismatch>) {
    mismatches.push(ComparisonMismatch {
        path: path.to_string().into(),
        expected: SemanticValue::from(expected.into()),
        actual: SemanticValue::from(actual.into()),
    });
}

fn comparison_error(path: &str, message: &str, errors: &mut Vec<ComparisonDiagnostic>) {
    errors.push(ComparisonDiagnostic {
        path: path.to_string().into(),
        message: message.to_string().into(),
    });
}
