use super::model::{AnyValue, CTSC_EVENTS, CTSC_SPANS, CTSC_VERSION, F64Value, TraceDocument, TraceEvent, TraceSpan};
use super::otlp::{self, ParsedDouble};
use super::{Loaded, ValidationIssue, issue, located, read_bytes};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const REQUIRED_RESOURCE_ATTRIBUTES: [&str; 5] = [
    "conformance.version",
    "conformance.tool.name",
    "conformance.tool.version",
    "conformance.target.name",
    "conformance.target.language",
];
const ANY_VALUE_KEYS: [&str; 7] = [
    "stringValue",
    "boolValue",
    "intValue",
    "doubleValue",
    "bytesValue",
    "arrayValue",
    "kvlistValue",
];
const RESOURCE_ATTRIBUTES: [&str; 9] = [
    "conformance.version",
    "conformance.tool.name",
    "conformance.tool.version",
    "conformance.target.name",
    "conformance.target.language",
    "conformance.registry.id",
    "conformance.registry.version",
    "conformance.registry.digest",
    "conformance.registry.uri",
];
const RUN_ATTRIBUTES: [&str; 2] = ["conformance.run.id", "conformance.run.name"];
const SCENARIO_ATTRIBUTES: [&str; 2] = ["conformance.scenario.name", "conformance.scenario.index"];
const OPERATION_ATTRIBUTES: [&str; 3] = [
    "conformance.component.id",
    "conformance.operation.name",
    "conformance.operation.inputs",
];
const PARALLEL_ATTRIBUTES: [&str; 1] = ["conformance.parallel.name"];
const OBSERVATION_ATTRIBUTES: [&str; 2] = ["conformance.observation.name", "conformance.observation.value"];
const RESULT_ATTRIBUTES: [&str; 1] = ["conformance.result.value"];
const ERROR_ATTRIBUTES: [&str; 2] = ["conformance.error.name", "conformance.error.value"];
const FAULT_ATTRIBUTES: [&str; 10] = [
    "conformance.fault.type",
    "conformance.fault.message",
    "conformance.fault.native_type",
    "conformance.fault.observer",
    "conformance.fault.phase",
    "conformance.fault.operation.name",
    "conformance.fault.operation.component_id",
    "conformance.fault.exit_code",
    "conformance.fault.signal",
    "conformance.fault.timeout_ms",
];
const CORE_SUPERVISOR_FAULT_TYPES: [&str; 7] = [
    "launch_failure",
    "process_exit",
    "signal",
    "timeout",
    "deadlock",
    "host_loss",
    "export_failure",
];

pub(crate) fn load_trace(path: &Path) -> Loaded<TraceDocument> {
    let mut issues = Vec::new();
    let Some(bytes) = read_bytes(path, &mut issues) else {
        return Loaded { value: None, issues };
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => {
            issue(&mut issues, located(path, "$"), format!("trace must be UTF-8: {error}"));
            return Loaded { value: None, issues };
        }
    };
    let documents = parse_documents(text, path, &mut issues);
    let parsed_any_document = !documents.is_empty();
    let mut spans = Vec::new();
    for (batch_index, value) in documents.iter().enumerate() {
        let Some(batch) = value.as_object() else {
            issue(
                &mut issues,
                located(path, &format!("$[{batch_index}]")),
                "OTLP TracesData must be a JSON object",
            );
            continue;
        };
        for (resource_index, resource_value) in array_field(batch, "resourceSpans", path, &format!("$[{batch_index}]"), &mut issues)
            .iter()
            .enumerate()
        {
            let resource_location = format!("$[{batch_index}].resourceSpans[{resource_index}]");
            let Some(resource_spans) = resource_value.as_object() else {
                issue(
                    &mut issues,
                    located(path, &resource_location),
                    "resourceSpans item must be an object",
                );
                continue;
            };
            let mut raw_spans = Vec::new();
            for (scope_index, scope_value) in array_field(resource_spans, "scopeSpans", path, &resource_location, &mut issues)
                .iter()
                .enumerate()
            {
                let scope_location = format!("{resource_location}.scopeSpans[{scope_index}]");
                let Some(scope) = scope_value.as_object() else {
                    issue(&mut issues, located(path, &scope_location), "scopeSpans item must be an object");
                    continue;
                };
                let mut scope_spans = Vec::new();
                for (span_index, span_value) in array_field(scope, "spans", path, &scope_location, &mut issues).iter().enumerate() {
                    let span_location = format!("{scope_location}.spans[{span_index}]");
                    let Some(span) = span_value.as_object() else {
                        issue(&mut issues, located(path, &span_location), "span must be an object");
                        continue;
                    };
                    if span
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| CTSC_SPANS.contains(&name))
                    {
                        scope_spans.push((span, span_location));
                    }
                }
                if !scope_spans.is_empty() {
                    if let Some(scope_value) = json_field(scope, "scope") {
                        if let Some(instrumentation_scope) = scope_value.as_object() {
                            validate_zero_count(
                                instrumentation_scope,
                                "droppedAttributesCount",
                                path,
                                &format!("{scope_location}.scope"),
                                &mut issues,
                            );
                            let attributes = parse_attributes(
                                json_field(instrumentation_scope, "attributes"),
                                path,
                                &format!("{scope_location}.scope"),
                                &mut issues,
                            );
                            reject_unknown_conformance_attributes(
                                &attributes,
                                &[],
                                &located(path, &format!("{scope_location}.scope")),
                                &mut issues,
                            );
                        } else if !scope_value.is_null() {
                            issue(
                                &mut issues,
                                located(path, &format!("{scope_location}.scope")),
                                "scope must be an object",
                            );
                        }
                    }
                    raw_spans.extend(scope_spans);
                }
            }
            if raw_spans.is_empty() {
                continue;
            }
            let resource_value = json_field(resource_spans, "resource");
            let resource = resource_value.and_then(Value::as_object);
            if resource_value.is_some_and(|value| !value.is_object() && !value.is_null()) {
                issue(
                    &mut issues,
                    located(path, &format!("{resource_location}.resource")),
                    "resource must be an object",
                );
            }
            if let Some(resource) = resource {
                validate_zero_count(
                    resource,
                    "droppedAttributesCount",
                    path,
                    &format!("{resource_location}.resource"),
                    &mut issues,
                );
            }
            let resource_attributes = parse_attributes(
                resource.and_then(|value| json_field(value, "attributes")),
                path,
                &format!("{resource_location}.resource"),
                &mut issues,
            );
            reject_unknown_conformance_attributes(
                &resource_attributes,
                &RESOURCE_ATTRIBUTES,
                &located(path, &format!("{resource_location}.resource")),
                &mut issues,
            );
            for key in REQUIRED_RESOURCE_ATTRIBUTES {
                let actual = require_string_attribute(
                    &resource_attributes,
                    key,
                    path,
                    &format!("{resource_location}.resource"),
                    &mut issues,
                );
                if key == "conformance.version" && actual.is_some_and(|value| value != CTSC_VERSION) {
                    issue(
                        &mut issues,
                        located(path, &format!("{resource_location}.resource")),
                        "conformance.version must be '0.2.0'",
                    );
                }
            }
            for key in [
                "conformance.registry.id",
                "conformance.registry.version",
                "conformance.registry.digest",
                "conformance.registry.uri",
            ] {
                validate_optional_attribute_type(
                    &resource_attributes,
                    key,
                    AttributeType::String,
                    &located(path, &format!("{resource_location}.resource")),
                    &mut issues,
                );
            }
            for (raw, location) in raw_spans {
                if let Some(span) = parse_span(raw, resource_attributes.clone(), path, &location, &mut issues) {
                    spans.push(span);
                }
            }
        }
    }
    if spans.is_empty() && (parsed_any_document || text.trim().is_empty()) {
        issue(&mut issues, located(path, "$"), "trace contains no CTSC spans");
    }
    validate_semantics(&spans, path, &mut issues);
    Loaded {
        value: Some(TraceDocument { spans }),
        issues,
    }
}

fn parse_documents(text: &str, path: &Path, issues: &mut Vec<ValidationIssue>) -> Vec<Value> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
        return match otlp::parse_document(text) {
            Ok(value) => vec![value],
            Err(error) => {
                issue(issues, located(path, "$"), format!("invalid OTLP JSON/protobuf mapping: {error}"));
                Vec::new()
            }
        };
    }
    let mut documents = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            issue(&mut *issues, located(path, &format!("line {line_number}")), "blank JSONL line");
            continue;
        }
        match otlp::parse_document(line) {
            Ok(value) => documents.push(value),
            Err(error) => issue(
                issues,
                located(path, &format!("line {line_number}")),
                format!("invalid OTLP JSON/protobuf mapping: {error}"),
            ),
        }
    }
    documents
}

fn parse_span(
    value: &Map<String, Value>,
    resource_attributes: BTreeMap<String, AnyValue>,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<TraceSpan> {
    for key in ["droppedAttributesCount", "droppedEventsCount", "droppedLinksCount"] {
        validate_zero_count(value, key, path, location, issues);
    }
    let trace_id = hex_id_field(value, "traceId", path, location, issues)?;
    let span_id = hex_id_field(value, "spanId", path, location, issues)?;
    let parent_span_id = optional_hex_id_field(value, "parentSpanId", path, location, issues).unwrap_or_default();
    let name = string_field(value, "name", path, location, issues)?;
    let start_time = time_field(value, "startTimeUnixNano", path, location, issues);
    let end_time = time_field(value, "endTimeUnixNano", path, location, issues);
    let attributes = parse_attributes(json_field(value, "attributes"), path, location, issues);
    let mut events = Vec::new();
    for (event_index, event_value) in array_field(value, "events", path, location, issues).iter().enumerate() {
        let event_location = format!("{location}.events[{event_index}]");
        let Some(event) = event_value.as_object() else {
            issue(&mut *issues, located(path, &event_location), "event must be an object");
            continue;
        };
        validate_zero_count(event, "droppedAttributesCount", path, &event_location, issues);
        let Some(event_name) = string_field(event, "name", path, &event_location, issues) else {
            continue;
        };
        events.push(TraceEvent {
            name: event_name,
            attributes: parse_attributes(json_field(event, "attributes"), path, &event_location, issues),
        });
    }
    let status_error = json_field(value, "status")
        .and_then(Value::as_object)
        .and_then(|status| json_field(status, "code"))
        .is_some_and(|code| code.as_i64() == Some(2) || code.as_str() == Some("STATUS_CODE_ERROR"));
    Some(TraceSpan {
        trace_id,
        span_id,
        parent_span_id,
        name,
        start_time,
        end_time,
        attributes,
        events,
        status_error,
        resource_attributes,
        location: located(path, location),
    })
}

fn validate_semantics(spans: &[TraceSpan], path: &Path, issues: &mut Vec<ValidationIssue>) {
    let mut by_id = BTreeMap::<(&str, &str), Vec<&TraceSpan>>::new();
    for span in spans {
        require(
            is_hex_id(&span.trace_id, 32),
            &span.location,
            "traceId must be 32 lowercase hexadecimal characters and nonzero",
            issues,
        );
        require(
            is_hex_id(&span.span_id, 16),
            &span.location,
            "spanId must be 16 lowercase hexadecimal characters and nonzero",
            issues,
        );
        let candidates = by_id.entry((span.trace_id.as_str(), span.span_id.as_str())).or_default();
        if !candidates.is_empty() {
            issue(issues, span.location.clone(), "duplicate span ID within trace");
        }
        candidates.push(span);
    }
    validate_ancestry(spans, &by_id, issues);
    for span in spans {
        let parent_candidates = by_id.get(&(span.trace_id.as_str(), span.parent_span_id.as_str()));
        if parent_candidates.is_some_and(|candidates| candidates.len() > 1) {
            issue(issues, span.location.clone(), "parent span ID resolves to multiple CTSC spans");
        }
        let parent = parent_candidates.and_then(|candidates| (candidates.len() == 1).then_some(candidates[0]));
        let parent_name = parent.map(|value| value.name.as_str());
        require(
            span.start_time.zip(span.end_time).is_some_and(|(start, end)| start <= end),
            &span.location,
            "CTSC spans require start/end timestamps with end not before start",
            issues,
        );
        match span.name.as_str() {
            "conformance.run" => {
                reject_unknown_conformance_attributes(&span.attributes, &RUN_ATTRIBUTES, &span.location, issues);
                require(
                    span.parent_span_id.is_empty(),
                    &span.location,
                    "run span must be a root span",
                    issues,
                );
                require_string_attribute(&span.attributes, "conformance.run.id", path, &span.location, issues);
                validate_optional_attribute_type(
                    &span.attributes,
                    "conformance.run.name",
                    AttributeType::String,
                    &span.location,
                    issues,
                );
            }
            "conformance.scenario" => {
                reject_unknown_conformance_attributes(&span.attributes, &SCENARIO_ATTRIBUTES, &span.location, issues);
                require(
                    parent_name == Some("conformance.run"),
                    &span.location,
                    "scenario parent must be a conformance.run span",
                    issues,
                );
                require_string_attribute(&span.attributes, "conformance.scenario.name", path, &span.location, issues);
                validate_optional_attribute_type(
                    &span.attributes,
                    "conformance.scenario.index",
                    AttributeType::Int,
                    &span.location,
                    issues,
                );
            }
            "conformance.operation" => {
                reject_unknown_conformance_attributes(&span.attributes, &OPERATION_ATTRIBUTES, &span.location, issues);
                require(
                    matches!(
                        parent_name,
                        Some("conformance.scenario" | "conformance.operation" | "conformance.parallel")
                    ),
                    &span.location,
                    "operation parent must be scenario, operation, or parallel",
                    issues,
                );
                require_string_attribute(&span.attributes, "conformance.component.id", path, &span.location, issues);
                require_string_attribute(&span.attributes, "conformance.operation.name", path, &span.location, issues);
                require(
                    span.attributes
                        .get("conformance.operation.inputs")
                        .is_some_and(|value| matches!(value, AnyValue::KvList(_))),
                    &span.location,
                    "operation inputs must use kvlistValue",
                    issues,
                );
            }
            "conformance.parallel" => {
                reject_unknown_conformance_attributes(&span.attributes, &PARALLEL_ATTRIBUTES, &span.location, issues);
                require(
                    matches!(parent_name, Some("conformance.scenario" | "conformance.operation")),
                    &span.location,
                    "parallel parent must be scenario or operation",
                    issues,
                );
                validate_optional_attribute_type(
                    &span.attributes,
                    "conformance.parallel.name",
                    AttributeType::String,
                    &span.location,
                    issues,
                );
            }
            _ => {}
        }
        if let Some(parent) = parent
            && parent.name == "conformance.parallel"
        {
            require(
                parent
                    .start_time
                    .zip(parent.end_time)
                    .zip(span.start_time.zip(span.end_time))
                    .is_some_and(|((parent_start, parent_end), (child_start, child_end))| {
                        parent_start <= child_start && child_end <= parent_end
                    }),
                &span.location,
                "conformance.parallel interval must enclose every direct branch",
                issues,
            );
        }
        validate_events(span, path, issues);
    }
}

fn validate_events(span: &TraceSpan, path: &Path, issues: &mut Vec<ValidationIssue>) {
    let mut result_count = 0;
    let mut other_terminal_count = 0;
    let mut terminated = false;
    for (index, event) in span.events.iter().enumerate() {
        let location = format!("{}.events[{index}]", span.location);
        if span.name == "conformance.operation" && terminated {
            issue(
                issues,
                location.clone(),
                "operation events must not appear after the terminal event",
            );
        }
        if !CTSC_EVENTS.contains(&event.name.as_str()) {
            issue(issues, location, format!("unsupported CTSC event name '{}'", event.name));
            continue;
        }
        if matches!(
            event.name.as_str(),
            "conformance.observation" | "conformance.result" | "conformance.empty" | "conformance.error"
        ) {
            require(
                span.name == "conformance.operation",
                &location,
                &format!("{} must belong to a conformance.operation span", event.name),
                issues,
            );
        }
        match event.name.as_str() {
            "conformance.observation" => {
                reject_unknown_conformance_attributes(&event.attributes, &OBSERVATION_ATTRIBUTES, &location, issues);
                require_string_attribute(&event.attributes, "conformance.observation.name", path, &location, issues);
                require(
                    event.attributes.contains_key("conformance.observation.value"),
                    &location,
                    "missing conformance.observation.value",
                    issues,
                );
            }
            "conformance.result" => {
                reject_unknown_conformance_attributes(&event.attributes, &RESULT_ATTRIBUTES, &location, issues);
                result_count += 1;
                terminated = true;
                require(
                    event.attributes.contains_key("conformance.result.value"),
                    &location,
                    "missing conformance.result.value",
                    issues,
                );
            }
            "conformance.empty" => {
                reject_unknown_conformance_attributes(&event.attributes, &[], &location, issues);
                other_terminal_count += 1;
                terminated = true;
            }
            "conformance.error" => {
                reject_unknown_conformance_attributes(&event.attributes, &ERROR_ATTRIBUTES, &location, issues);
                other_terminal_count += 1;
                terminated = true;
                require_string_attribute(&event.attributes, "conformance.error.name", path, &location, issues);
            }
            "conformance.fault" => {
                reject_unknown_conformance_attributes(&event.attributes, &FAULT_ATTRIBUTES, &location, issues);
                other_terminal_count += usize::from(span.name == "conformance.operation");
                terminated |= span.name == "conformance.operation";
                require(
                    matches!(
                        span.name.as_str(),
                        "conformance.run" | "conformance.scenario" | "conformance.operation"
                    ),
                    &location,
                    "fault must belong to a run, scenario, or operation span",
                    issues,
                );
                let fault_type = require_string_attribute(&event.attributes, "conformance.fault.type", path, &location, issues);
                let observer = require_string_attribute(&event.attributes, "conformance.fault.observer", path, &location, issues);
                if observer == Some("target") {
                    require(
                        span.name == "conformance.operation",
                        &location,
                        "target fault must belong to an operation span",
                        issues,
                    );
                } else if observer == Some("supervisor") {
                    require(
                        matches!(span.name.as_str(), "conformance.run" | "conformance.scenario"),
                        &location,
                        "supervisor fault must belong to a run or scenario span",
                        issues,
                    );
                    if let Some(fault_type) = fault_type {
                        require(
                            CORE_SUPERVISOR_FAULT_TYPES.contains(&fault_type) || is_namespaced_fault_type(fault_type),
                            &location,
                            "supervisor fault type must be a defined core type or a producer-qualified dotted namespace",
                            issues,
                        );
                    }
                }
                for key in [
                    "conformance.fault.message",
                    "conformance.fault.native_type",
                    "conformance.fault.phase",
                    "conformance.fault.operation.name",
                    "conformance.fault.operation.component_id",
                    "conformance.fault.signal",
                ] {
                    validate_optional_attribute_type(&event.attributes, key, AttributeType::String, &location, issues);
                }
                for key in ["conformance.fault.exit_code", "conformance.fault.timeout_ms"] {
                    validate_optional_attribute_type(&event.attributes, key, AttributeType::Int, &location, issues);
                }
            }
            _ => {}
        }
    }
    if span.name == "conformance.operation" {
        require(
            other_terminal_count <= 1,
            &span.location,
            "operation must not contain multiple non-result completion/failure events",
            issues,
        );
        require(
            result_count == 0 || other_terminal_count == 0,
            &span.location,
            "result events cannot be combined with another completion/failure event",
            issues,
        );
        require(
            result_count <= 1,
            &span.location,
            "operation may contain at most one result event",
            issues,
        );
        if span
            .events
            .iter()
            .any(|event| matches!(event.name.as_str(), "conformance.error" | "conformance.fault"))
        {
            require(
                span.status_error,
                &span.location,
                "declared error and fault operations must have ERROR status",
                issues,
            );
        }
    }
    if matches!(span.name.as_str(), "conformance.run" | "conformance.scenario")
        && span.events.iter().any(|event| event.name == "conformance.fault")
    {
        require(
            span.status_error,
            &span.location,
            "fault-bearing run or scenario must have ERROR status",
            issues,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AncestryError {
    SelfParent,
    Cycle,
    MissingRun,
    MultipleParents,
}

fn validate_ancestry<'a>(
    spans: &'a [TraceSpan],
    by_id: &BTreeMap<(&'a str, &'a str), Vec<&'a TraceSpan>>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut resolved = Vec::new();
    let mut roots_by_trace = BTreeMap::<&str, BTreeSet<&str>>::new();
    for span in spans.iter().filter(|span| span.name != "conformance.run") {
        match run_ancestor(span, by_id) {
            Ok(run) => {
                roots_by_trace
                    .entry(span.trace_id.as_str())
                    .or_default()
                    .insert(run.span_id.as_str());
                resolved.push((span, run.span_id.as_str()));
            }
            Err(AncestryError::SelfParent) => {
                issue(issues, span.location.clone(), "CTSC ancestor chain contains a self-parent cycle");
            }
            Err(AncestryError::Cycle) => {
                issue(issues, span.location.clone(), "CTSC ancestor chain contains a cycle");
            }
            Err(AncestryError::MissingRun) => {
                issue(
                    issues,
                    span.location.clone(),
                    "CTSC ancestor chain must terminate at a conformance.run span",
                );
            }
            Err(AncestryError::MultipleParents) => {
                issue(
                    issues,
                    span.location.clone(),
                    "CTSC ancestor chain has invalid multiple ancestry because a parent span ID resolves to multiple CTSC spans",
                );
            }
        }
    }
    for (span, _) in resolved {
        if roots_by_trace.get(span.trace_id.as_str()).is_some_and(|roots| roots.len() > 1) {
            issue(
                issues,
                span.location.clone(),
                "CTSC spans sharing a traceId must terminate at the same conformance.run span",
            );
        }
    }
}

fn run_ancestor<'a>(span: &'a TraceSpan, by_id: &BTreeMap<(&'a str, &'a str), Vec<&'a TraceSpan>>) -> Result<&'a TraceSpan, AncestryError> {
    let mut current = span;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(current.span_id.as_str()) {
            return Err(AncestryError::Cycle);
        }
        if current.name == "conformance.run" {
            return current
                .parent_span_id
                .is_empty()
                .then_some(current)
                .ok_or(AncestryError::MissingRun);
        }
        if current.parent_span_id == current.span_id {
            return Err(AncestryError::SelfParent);
        }
        if current.parent_span_id.is_empty() {
            return Err(AncestryError::MissingRun);
        }
        let Some(parents) = by_id.get(&(current.trace_id.as_str(), current.parent_span_id.as_str())) else {
            return Err(AncestryError::MissingRun);
        };
        let [parent] = parents.as_slice() else {
            return Err(AncestryError::MultipleParents);
        };
        current = parent;
    }
}

fn is_namespaced_fault_type(value: &str) -> bool {
    let mut segments = value.split('.');
    let first = segments.next().unwrap_or_default();
    let remaining = segments.collect::<Vec<_>>();
    let valid_first = first != "conformance"
        && first.bytes().enumerate().all(|(index, byte)| {
            (index == 0 && byte.is_ascii_lowercase()) || (index > 0 && (byte.is_ascii_lowercase() || byte.is_ascii_digit()))
        });
    let valid_remaining = !remaining.is_empty()
        && remaining.iter().all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
        });
    !first.is_empty() && valid_first && valid_remaining
}

fn parse_attributes(value: Option<&Value>, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> BTreeMap<String, AnyValue> {
    let mut result = BTreeMap::new();
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return result;
    };
    let Some(attributes) = value.as_array() else {
        issue(
            issues,
            located(path, &format!("{location}.attributes")),
            "attributes must be an array",
        );
        return result;
    };
    for (index, attribute) in attributes.iter().enumerate() {
        let item_location = format!("{location}.attributes[{index}]");
        let Some(attribute) = attribute.as_object() else {
            issue(issues, located(path, &item_location), "attribute must be an object");
            continue;
        };
        let Some(key) = json_field(attribute, "key").and_then(Value::as_str) else {
            issue(issues, located(path, &item_location), "attribute key must be a string");
            continue;
        };
        if result.contains_key(key) {
            issue(issues, located(path, location), format!("duplicate attribute key '{key}'"));
            continue;
        }
        let Some(raw_value) = json_field(attribute, "value") else {
            issue(
                issues,
                located(path, &format!("{location}.{key}")),
                "attribute value must be an AnyValue",
            );
            continue;
        };
        if let Some(value) = parse_any_value(raw_value, path, &format!("{location}.{key}"), issues) {
            result.insert(key.to_string(), value);
        }
    }
    result
}

fn parse_any_value(value: &Value, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> Option<AnyValue> {
    let Some(value) = value.as_object() else {
        issue(issues, located(path, location), "AnyValue must be an object");
        return None;
    };
    let selected = ANY_VALUE_KEYS
        .iter()
        .filter(|key| json_field(value, key).is_some())
        .copied()
        .collect::<Vec<_>>();
    if selected.len() != 1 || value.len() != 1 {
        issue(
            issues,
            located(path, location),
            "AnyValue must select exactly one concrete value variant",
        );
        return None;
    }
    let raw = json_field(value, selected[0]).expect("selected AnyValue field");
    match selected[0] {
        "stringValue" => raw.as_str().map(|value| AnyValue::String(value.to_string())).or_else(|| {
            issue(issues, located(path, location), "stringValue must be a string");
            None
        }),
        "boolValue" => raw.as_bool().map(AnyValue::Bool).or_else(|| {
            issue(issues, located(path, location), "boolValue must be a boolean");
            None
        }),
        "intValue" => otlp::parse_i64(raw).map(AnyValue::Int).or_else(|| {
            issue(
                issues,
                located(path, location),
                "intValue must contain an OTLP int64 JSON number or numeric string",
            );
            None
        }),
        "doubleValue" => parse_double(raw, path, location, issues).map(AnyValue::Double),
        "bytesValue" => raw.as_str().and_then(otlp::decode_base64).map(AnyValue::Bytes).or_else(|| {
            issue(issues, located(path, location), "bytesValue must be valid base64");
            None
        }),
        "arrayValue" => {
            let Some(array) = raw.as_object() else {
                issue(issues, located(path, location), "arrayValue must be an object");
                return None;
            };
            let values = optional_array(array, "values", path, location, issues)?;
            let mut parsed = Vec::new();
            for (index, item) in values.iter().enumerate() {
                if let Some(item) = parse_any_value(item, path, &format!("{location}[{index}]"), issues) {
                    parsed.push(item);
                }
            }
            Some(AnyValue::Array(parsed))
        }
        "kvlistValue" => {
            let Some(kvlist) = raw.as_object() else {
                issue(issues, located(path, location), "kvlistValue must be an object");
                return None;
            };
            let values = optional_array(kvlist, "values", path, location, issues)?;
            let mut parsed = BTreeMap::new();
            for (index, item) in values.iter().enumerate() {
                let item_location = format!("{location}.{index}");
                let Some(item) = item.as_object() else {
                    issue(issues, located(path, &item_location), "kvlist item must be an object");
                    continue;
                };
                let Some(key) = json_field(item, "key").and_then(Value::as_str) else {
                    issue(issues, located(path, &item_location), "kvlist item must contain a string key");
                    continue;
                };
                if parsed.contains_key(key) {
                    issue(issues, located(path, location), format!("duplicate kvlist key '{key}'"));
                    continue;
                }
                let Some(child) = json_field(item, "value") else {
                    issue(issues, located(path, &item_location), "kvlist item must contain an AnyValue");
                    continue;
                };
                if let Some(child) = parse_any_value(child, path, &format!("{location}.{key}"), issues) {
                    parsed.insert(key.to_string(), child);
                }
            }
            Some(AnyValue::KvList(parsed))
        }
        _ => unreachable!(),
    }
}

fn parse_double(value: &Value, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> Option<F64Value> {
    match otlp::parse_double(value) {
        Some(ParsedDouble::Finite(value)) => Some(F64Value::Finite(value.to_bits())),
        Some(ParsedDouble::NaN) => Some(F64Value::NaN),
        Some(ParsedDouble::Infinity) => Some(F64Value::Infinity),
        Some(ParsedDouble::NegativeInfinity) => Some(F64Value::NegativeInfinity),
        None => {
            issue(
                issues,
                located(path, location),
                "doubleValue must be an OTLP number or numeric/symbolic string",
            );
            None
        }
    }
}

fn array_field<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> &'a [Value] {
    match json_field(object, key) {
        None | Some(Value::Null) => &[],
        Some(Value::Array(values)) => values,
        Some(_) => {
            issue(
                issues,
                located(path, &format!("{location}.{key}")),
                format!("{key} must be an array"),
            );
            &[]
        }
    }
}

fn optional_array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<&'a [Value]> {
    match json_field(object, key) {
        None | Some(Value::Null) => Some(&[]),
        Some(Value::Array(values)) => Some(values),
        Some(_) => {
            issue(
                issues,
                located(path, location),
                format!("{} must contain a values array", if key == "values" { "value" } else { key }),
            );
            None
        }
    }
}

fn json_field<'a>(object: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    object.get(key).or_else(|| {
        let snake_case = key.chars().fold(String::new(), |mut result, character| {
            if character.is_ascii_uppercase() {
                result.push('_');
                result.push(character.to_ascii_lowercase());
            } else {
                result.push(character);
            }
            result
        });
        object.get(&snake_case)
    })
}

fn string_field(object: &Map<String, Value>, key: &str, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> Option<String> {
    if let Some(value) = json_field(object, key).and_then(Value::as_str) {
        Some(value.to_string())
    } else {
        issue(
            issues,
            located(path, &format!("{location}.{key}")),
            format!("{key} must be a string"),
        );
        None
    }
}

fn optional_string_field(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    match json_field(object, key) {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            issue(
                issues,
                located(path, &format!("{location}.{key}")),
                format!("{key} must be a string"),
            );
            None
        }
    }
}

fn hex_id_field(object: &Map<String, Value>, key: &str, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> Option<String> {
    let value = string_field(object, key, path, location, issues)?;
    otlp::normalize_hex(&value).or_else(|| {
        issue(
            issues,
            located(path, &format!("{location}.{key}")),
            format!("{key} must use the OTLP hexadecimal bytes mapping"),
        );
        None
    })
}

fn optional_hex_id_field(
    object: &Map<String, Value>,
    key: &str,
    path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<String> {
    let value = optional_string_field(object, key, path, location, issues)?;
    otlp::normalize_hex(&value).or_else(|| {
        issue(
            issues,
            located(path, &format!("{location}.{key}")),
            format!("{key} must use the OTLP hexadecimal bytes mapping"),
        );
        None
    })
}

fn time_field(object: &Map<String, Value>, key: &str, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) -> Option<u128> {
    let value = json_field(object, key)?;
    if value.is_null() {
        return None;
    }
    let parsed = otlp::parse_u64(value).map(u128::from);
    if parsed.is_none() {
        issue(
            issues,
            located(path, &format!("{location}.{key}")),
            format!("{key} must be an unsigned decimal integer"),
        );
    }
    parsed
}

fn validate_zero_count(object: &Map<String, Value>, key: &str, path: &Path, location: &str, issues: &mut Vec<ValidationIssue>) {
    let Some(value) = json_field(object, key).filter(|value| !value.is_null()) else {
        return;
    };
    let parsed = otlp::parse_u32(value);
    require(
        parsed == Some(0),
        &located(path, location),
        &format!("{key} must be a valid uint32 zero"),
        issues,
    );
}

#[derive(Clone, Copy)]
enum AttributeType {
    String,
    Int,
}

fn reject_unknown_conformance_attributes(
    attributes: &BTreeMap<String, AnyValue>,
    allowed: &[&str],
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    for key in attributes.keys().filter(|key| key.starts_with("conformance.")) {
        if !allowed.contains(&key.as_str()) {
            issue(
                issues,
                location.to_string(),
                format!("undeclared CTSC attribute '{key}' is not allowed in this scope"),
            );
        }
    }
}

fn validate_optional_attribute_type(
    attributes: &BTreeMap<String, AnyValue>,
    key: &str,
    expected: AttributeType,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let Some(value) = attributes.get(key) else {
        return;
    };
    let valid = match expected {
        AttributeType::String => matches!(value, AnyValue::String(_)),
        AttributeType::Int => matches!(value, AnyValue::Int(_)),
    };
    if !valid {
        let variant = match expected {
            AttributeType::String => "stringValue",
            AttributeType::Int => "intValue",
        };
        issue(issues, location.to_string(), format!("attribute '{key}' must use {variant}"));
    }
}

fn require_string_attribute<'a>(
    attributes: &'a BTreeMap<String, AnyValue>,
    key: &str,
    _path: &Path,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<&'a str> {
    match attributes.get(key) {
        Some(AnyValue::String(value)) => Some(value),
        Some(_) => {
            issue(issues, location.to_string(), format!("attribute '{key}' must use stringValue"));
            None
        }
        None => {
            issue(issues, location.to_string(), format!("missing string attribute '{key}'"));
            None
        }
    }
}

fn is_hex_id(value: &str, length: usize) -> bool {
    value.len() == length
        && value.bytes().any(|byte| byte != b'0')
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn require(condition: bool, location: &str, message: &str, issues: &mut Vec<ValidationIssue>) {
    if !condition {
        issue(issues, location.to_string(), message);
    }
}
