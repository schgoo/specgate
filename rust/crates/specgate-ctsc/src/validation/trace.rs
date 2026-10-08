//! Parse and validate CTSC Trace Core OTLP JSON and JSONL while collecting stable diagnostics.

use super::model::{AnyValue, CTSC_EVENTS, CTSC_SPANS, CTSC_VERSION, F64Value, FiniteF64, TraceDocument, TraceEvent, TraceSpan};
use super::otlp::{self, ParsedDouble};
use super::{Loaded, ValidationIssue, issue, located};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

mod ancestry;
mod fault;
mod fields;
use ancestry::validate_ancestry;
use fault::is_fault;
use fields::{
    AttributeType, array_field, check_attr, id_field, is_hex_id, json_field, optional_id, parse_attributes, reject_attrs, require,
    require_str, string_field, time_field, validate_zero,
};

// OTLP StatusCode's fixed protobuf value for STATUS_CODE_ERROR. Producers and
// validators must preserve this numeric mapping for interoperable JSON.
const STATUS_ERROR: i64 = 2;
// OTLP trace and span identifiers are 16 and 8 bytes respectively, encoded
// as two lowercase hexadecimal characters per byte in the JSON mapping.
const TRACE_ID_LEN: usize = 32;
const SPAN_ID_LEN: usize = 16;
const REQUIRED_ATTRS: [&str; 5] = [
    "conformance.version",
    "conformance.tool.name",
    "conformance.tool.version",
    "conformance.target.name",
    "conformance.target.language",
];
const VALUE_KEYS: [&str; 7] = [
    "stringValue",
    "boolValue",
    "intValue",
    "doubleValue",
    "bytesValue",
    "arrayValue",
    "kvlistValue",
];
const RESOURCE_ATTRS: [&str; 9] = [
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
const RUN_ATTRS: [&str; 2] = ["conformance.run.id", "conformance.run.name"];
const SCENARIO_ATTRS: [&str; 2] = ["conformance.scenario.name", "conformance.scenario.index"];
const OP_ATTRS: [&str; 3] = [
    "conformance.component.id",
    "conformance.operation.name",
    "conformance.operation.inputs",
];
const PARALLEL_ATTRS: [&str; 1] = ["conformance.parallel.name"];
const OBS_ATTRS: [&str; 2] = ["conformance.observation.name", "conformance.observation.value"];
const RESULT_ATTRS: [&str; 1] = ["conformance.result.value"];
const ERROR_ATTRS: [&str; 2] = ["conformance.error.name", "conformance.error.value"];
const FAULT_ATTRS: [&str; 10] = [
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
const SUPERVISOR_FAULTS: [&str; 7] = [
    "launch_failure",
    "process_exit",
    "signal",
    "timeout",
    "deadlock",
    "host_loss",
    "export_failure",
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanRole {
    Run,
    Scenario,
    Operation,
    Parallel,
    Other,
}
impl SpanRole {
    fn parse(value: impl AsRef<str>) -> Self {
        match value.as_ref() {
            "conformance.run" => Self::Run,
            "conformance.scenario" => Self::Scenario,
            "conformance.operation" => Self::Operation,
            "conformance.parallel" => Self::Parallel,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EventRole {
    Observation,
    Result,
    Empty,
    Error,
    Fault,
    Other,
}
impl EventRole {
    fn parse(value: impl AsRef<str>) -> Self {
        match value.as_ref() {
            "conformance.observation" => Self::Observation,
            "conformance.result" => Self::Result,
            "conformance.empty" => Self::Empty,
            "conformance.error" => Self::Error,
            "conformance.fault" => Self::Fault,
            _ => Self::Other,
        }
    }
    fn operation_only(self) -> bool {
        matches!(self, Self::Observation | Self::Result | Self::Empty | Self::Error)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FaultObserver {
    Target,
    Supervisor,
    Other,
}
impl FaultObserver {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("target") => Self::Target,
            Some("supervisor") => Self::Supervisor,
            _ => Self::Other,
        }
    }
}

/// Read a trace path and collect parse and semantic issues without failing fast.
pub(crate) fn load_trace(path: impl AsRef<Path>) -> Loaded<TraceDocument> {
    load_from(path.as_ref(), &crate::comparison::SystemReader::system())
}

/// Load and validate one trace through an injected document reader.
///
/// This is the testability seam for filesystem and read failures. It collects
/// all parse and semantic issues in the same form as [`load_trace`].
pub(crate) fn load_from(path: impl AsRef<Path>, reader: &impl crate::comparison::DocumentReader) -> Loaded<TraceDocument> {
    let path = path.as_ref();
    let mut issues = Vec::new();
    let Some(bytes) = super::finish_read(path, reader.read(path), &mut issues) else {
        return Loaded { value: None, issues };
    };
    load_bytes(path, &bytes)
}

/// Parse named trace bytes and collect all discoverable validation issues.
pub(crate) fn load_bytes(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> Loaded<TraceDocument> {
    let path = path.as_ref();
    let bytes = bytes.as_ref();
    let mut issues = Vec::new();
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            issue(located(path, "$"), format!("trace must be UTF-8: {error}"), &mut issues);
            return Loaded { value: None, issues };
        }
    };
    let documents = parse_documents(path, text, &mut issues);
    let has_documents = !documents.is_empty();
    let mut spans = Vec::new();
    let mut resource_location = String::with_capacity("$[].resourceSpans[]".len() + 2 * (usize::MAX.ilog10() as usize + 1));
    let mut resource_field = String::with_capacity(resource_location.capacity() + ".resource".len());
    let mut raw_spans = Vec::new();
    let index_digits = usize::MAX.ilog10() as usize + 1;
    let mut scope_location = String::with_capacity(resource_location.capacity() + ".scopeSpans[]".len() + index_digits);
    let mut span_location = String::with_capacity(scope_location.capacity() + ".spans[]".len() + index_digits);
    let mut scope_spans = Vec::new();
    let mut scope_field = String::with_capacity(scope_location.capacity() + ".scope".len());
    let mut batch_location = String::with_capacity("$[]".len() + index_digits);
    for (batch_index, value) in documents.iter().enumerate() {
        let Some(batch) = value.as_object() else {
            issue(
                located(path, format!("$[{batch_index}]")),
                "OTLP TracesData must be a JSON object",
                &mut issues,
            );
            continue;
        };
        batch_location.clear();
        write!(batch_location, "$[{batch_index}]").expect("writing to a String cannot fail");
        for (resource_index, resource_value) in array_field(batch, "resourceSpans", path, &batch_location, &mut issues)
            .iter()
            .enumerate()
        {
            resource_location.clear();
            write!(resource_location, "$[{batch_index}].resourceSpans[{resource_index}]").expect("writing to a String cannot fail");
            let Some(resource_spans) = resource_value.as_object() else {
                issue(
                    located(path, &resource_location),
                    "resourceSpans item must be an object",
                    &mut issues,
                );
                continue;
            };
            raw_spans.clear();
            scope_location.clear();
            span_location.clear();
            scope_spans.clear();
            scope_field.clear();
            for (scope_index, scope_value) in array_field(resource_spans, "scopeSpans", path, &resource_location, &mut issues)
                .iter()
                .enumerate()
            {
                scope_location.clear();
                write!(scope_location, "{resource_location}.scopeSpans[{scope_index}]").expect("writing to a String cannot fail");
                let Some(scope) = scope_value.as_object() else {
                    issue(located(path, &scope_location), "scopeSpans item must be an object", &mut issues);
                    continue;
                };
                scope_spans.clear();
                scope_field.clear();
                write!(scope_field, "{scope_location}.scope").expect("writing to a String cannot fail");
                for (span_index, span_value) in array_field(scope, "spans", path, &scope_location, &mut issues).iter().enumerate() {
                    span_location.clear();
                    write!(span_location, "{scope_location}.spans[{span_index}]").expect("writing to a String cannot fail");
                    let Some(span) = span_value.as_object() else {
                        issue(located(path, &span_location), "span must be an object", &mut issues);
                        continue;
                    };
                    if span
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| CTSC_SPANS.contains(&name))
                    {
                        scope_spans.push((span, span_location.clone()));
                    }
                }
                if !scope_spans.is_empty() {
                    if let Some(scope_value) = json_field(scope, "scope") {
                        if let Some(instrumentation_scope) = scope_value.as_object() {
                            validate_zero(instrumentation_scope, "droppedAttributesCount", path, &scope_field, &mut issues);
                            let attributes =
                                parse_attributes(json_field(instrumentation_scope, "attributes"), path, &scope_field, &mut issues);
                            reject_attrs(&attributes, [], located(path, &scope_field), &mut issues);
                        } else if !scope_value.is_null() {
                            issue(located(path, &scope_field), "scope must be an object", &mut issues);
                        }
                    }
                    raw_spans.append(&mut scope_spans);
                }
            }
            if raw_spans.is_empty() {
                continue;
            }
            resource_field.clear();
            write!(resource_field, "{resource_location}.resource").expect("writing to a String cannot fail");
            let resource_value = json_field(resource_spans, "resource");
            let resource = resource_value.and_then(Value::as_object);
            if resource_value.is_some_and(|value| !value.is_object() && !value.is_null()) {
                issue(located(path, &resource_field), "resource must be an object", &mut issues);
            }
            if let Some(resource) = resource {
                validate_zero(resource, "droppedAttributesCount", path, &resource_field, &mut issues);
            }
            let resource_attributes = parse_attributes(
                resource.and_then(|value| json_field(value, "attributes")),
                path,
                &resource_field,
                &mut issues,
            );
            reject_attrs(&resource_attributes, RESOURCE_ATTRS, located(path, &resource_field), &mut issues);
            for key in REQUIRED_ATTRS {
                let actual = require_str(&resource_attributes, key, path, &resource_field, &mut issues);
                if key == "conformance.version" && actual.is_some_and(|value| value != CTSC_VERSION) {
                    issue(
                        located(path, &resource_field),
                        format!("conformance.version must be '{CTSC_VERSION}'"),
                        &mut issues,
                    );
                }
            }
            for key in [
                "conformance.registry.id",
                "conformance.registry.version",
                "conformance.registry.digest",
                "conformance.registry.uri",
            ] {
                check_attr(
                    &resource_attributes,
                    key,
                    AttributeType::String,
                    located(path, &resource_field),
                    &mut issues,
                );
            }
            for (raw, location) in raw_spans.drain(..) {
                let mut context = SpanContext {
                    path,
                    location: &location,
                    issues: &mut issues,
                };
                if let Some(span) = parse_span(raw, resource_attributes.clone(), &mut context) {
                    spans.push(span);
                }
            }
        }
    }
    if spans.is_empty() && (has_documents || text.trim().is_empty()) {
        issue(located(path, "$"), "trace contains no CTSC spans", &mut issues);
    }
    validate_semantics(&spans, path, &mut issues);
    spans.shrink_to_fit();
    issues.shrink_to_fit();
    Loaded {
        value: Some(TraceDocument { spans }),
        issues,
    }
}

fn parse_documents(path: impl AsRef<Path>, text: impl AsRef<str>, issues: &mut Vec<ValidationIssue>) -> Vec<Value> {
    let text = text.as_ref();
    let path = path.as_ref();
    if path.extension().and_then(|extension| extension.to_str()) != Some("jsonl") {
        return match otlp::parse_document(text) {
            Ok(value) => vec![value],
            Err(error) => {
                issue(located(path, "$"), format!("invalid OTLP JSON/protobuf mapping: {error}"), issues);
                Vec::new()
            }
        };
    }
    let mut documents = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            issue(located(path, format!("line {line_number}")), "blank JSONL line", &mut *issues);
            continue;
        }
        match otlp::parse_document(line) {
            Ok(value) => documents.push(value),
            Err(error) => issue(
                located(path, format!("line {line_number}")),
                format!("invalid OTLP JSON/protobuf mapping: {error}"),
                issues,
            ),
        }
    }
    documents
}

struct SpanContext<'a> {
    path: &'a Path,
    location: &'a str,
    issues: &'a mut Vec<ValidationIssue>,
}

fn parse_span(
    value: &Map<String, Value>,
    resource_attributes: BTreeMap<String, AnyValue>,
    context: &mut SpanContext<'_>,
) -> Option<TraceSpan> {
    let path = context.path;
    let location = context.location;
    let issues = &mut *context.issues;
    for key in ["droppedAttributesCount", "droppedEventsCount", "droppedLinksCount"] {
        validate_zero(value, key, path, location, issues);
    }
    let trace_id = id_field(value, "traceId", path, location, issues)?;
    let span_id = id_field(value, "spanId", path, location, issues)?;
    let parent_span_id = optional_id(value, "parentSpanId", path, location, issues).unwrap_or_default();
    let name = string_field(value, "name", path, location, issues)?;
    let start_time = time_field(value, "startTimeUnixNano", path, location, issues);
    let end_time = time_field(value, "endTimeUnixNano", path, location, issues);
    let attributes = parse_attributes(json_field(value, "attributes"), path, location, issues);
    let event_values = array_field(value, "events", path, location, issues);
    let mut events = Vec::with_capacity(event_values.len());
    let mut event_location = String::with_capacity(location.len() + ".events[]".len() + usize::MAX.ilog10() as usize + 1);
    for (event_index, event_value) in event_values.iter().enumerate() {
        event_location.clear();
        write!(event_location, "{location}.events[{event_index}]").expect("writing to a String cannot fail");
        let Some(event) = event_value.as_object() else {
            issue(located(path, &event_location), "event must be an object", &mut *issues);
            continue;
        };
        validate_zero(event, "droppedAttributesCount", path, &event_location, issues);
        let Some(event_name) = string_field(event, "name", path, &event_location, issues) else {
            continue;
        };
        events.push(TraceEvent {
            name: event_name.into(),
            attributes: parse_attributes(json_field(event, "attributes"), path, &event_location, issues),
        });
    }
    let status_error = json_field(value, "status")
        .and_then(Value::as_object)
        .and_then(|status| json_field(status, "code"))
        .is_some_and(|code| code.as_i64() == Some(STATUS_ERROR) || code.as_str() == Some("STATUS_CODE_ERROR"));
    events.shrink_to_fit();
    Some(TraceSpan {
        trace_id: trace_id.into(),
        span_id: span_id.into(),
        parent_span_id: parent_span_id.into(),
        name: name.into(),
        start_time,
        end_time,
        attributes,
        events: events.into_boxed_slice(),
        status_error,
        resource_attributes,
        location: located(path, location),
    })
}

fn validate_semantics(spans: impl AsRef<[TraceSpan]>, path: impl AsRef<Path>, issues: &mut Vec<ValidationIssue>) {
    let spans = spans.as_ref();
    let path = path.as_ref();
    let mut by_id = BTreeMap::<(&str, &str), Vec<&TraceSpan>>::new();
    for span in spans {
        require(
            is_hex_id(&span.trace_id, TRACE_ID_LEN),
            &span.location,
            "traceId must be 32 lowercase hexadecimal characters and nonzero",
            issues,
        );
        require(
            is_hex_id(&span.span_id, SPAN_ID_LEN),
            &span.location,
            "spanId must be 16 lowercase hexadecimal characters and nonzero",
            issues,
        );
        let candidates = by_id.entry((span.trace_id.as_str(), span.span_id.as_str())).or_default();
        if !candidates.is_empty() {
            issue(span.location.clone(), "duplicate span ID within trace", issues);
        }
        candidates.push(span);
    }
    validate_ancestry(spans, &by_id, issues);
    for span in spans {
        let parent_candidates = by_id.get(&(span.trace_id.as_str(), span.parent_span_id.as_str()));
        if parent_candidates.is_some_and(|candidates| candidates.len() > 1) {
            issue(span.location.clone(), "parent span ID resolves to multiple CTSC spans", issues);
        }
        let parent = parent_candidates.and_then(|candidates| (candidates.len() == 1).then_some(candidates[0]));
        let parent_role = parent.map_or(SpanRole::Other, |value| SpanRole::parse(value.name.as_str()));
        let span_role = SpanRole::parse(span.name.as_str());
        require(
            span.start_time.zip(span.end_time).is_some_and(|(start, end)| start <= end),
            &span.location,
            "CTSC spans require start/end timestamps with end not before start",
            issues,
        );
        match span_role {
            SpanRole::Run => {
                reject_attrs(&span.attributes, RUN_ATTRS, &span.location, issues);
                require(
                    span.parent_span_id.is_empty(),
                    &span.location,
                    "run span must be a root span",
                    issues,
                );
                require_str(&span.attributes, "conformance.run.id", path, &span.location, issues);
                check_attr(
                    &span.attributes,
                    "conformance.run.name",
                    AttributeType::String,
                    &span.location,
                    issues,
                );
            }
            SpanRole::Scenario => {
                reject_attrs(&span.attributes, SCENARIO_ATTRS, &span.location, issues);
                require(
                    parent_role == SpanRole::Run,
                    &span.location,
                    "scenario parent must be a conformance.run span",
                    issues,
                );
                require_str(&span.attributes, "conformance.scenario.name", path, &span.location, issues);
                check_attr(
                    &span.attributes,
                    "conformance.scenario.index",
                    AttributeType::Int,
                    &span.location,
                    issues,
                );
            }
            SpanRole::Operation => {
                reject_attrs(&span.attributes, OP_ATTRS, &span.location, issues);
                require(
                    matches!(parent_role, SpanRole::Scenario | SpanRole::Operation | SpanRole::Parallel),
                    &span.location,
                    "operation parent must be scenario, operation, or parallel",
                    issues,
                );
                require_str(&span.attributes, "conformance.component.id", path, &span.location, issues);
                require_str(&span.attributes, "conformance.operation.name", path, &span.location, issues);
                require(
                    span.attributes
                        .get("conformance.operation.inputs")
                        .is_some_and(|value| matches!(value, AnyValue::KvList(_))),
                    &span.location,
                    "operation inputs must use kvlistValue",
                    issues,
                );
            }
            SpanRole::Parallel => {
                reject_attrs(&span.attributes, PARALLEL_ATTRS, &span.location, issues);
                require(
                    matches!(parent_role, SpanRole::Scenario | SpanRole::Operation),
                    &span.location,
                    "parallel parent must be scenario or operation",
                    issues,
                );
                check_attr(
                    &span.attributes,
                    "conformance.parallel.name",
                    AttributeType::String,
                    &span.location,
                    issues,
                );
            }
            SpanRole::Other => {}
        }
        if let Some(parent) = parent
            && SpanRole::parse(parent.name.as_str()) == SpanRole::Parallel
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

fn validate_events(span: &TraceSpan, path: impl AsRef<Path>, issues: &mut Vec<ValidationIssue>) {
    let path = path.as_ref();
    let span_role = SpanRole::parse(span.name.as_str());
    let mut result_count = 0;
    let mut failure_count = 0;
    let mut terminated = false;
    let mut location = String::with_capacity(span.location.len() + ".events[]".len() + usize::MAX.ilog10() as usize + 1);
    for (index, event) in span.events.iter().enumerate() {
        location.clear();
        write!(location, "{}.events[{index}]", span.location).expect("writing to a String cannot fail");
        if span_role == SpanRole::Operation && terminated {
            issue(
                location.clone(),
                "operation events must not appear after the terminal event",
                issues,
            );
        }
        if !CTSC_EVENTS.contains(&event.name.as_str()) {
            issue(location.clone(), format!("unsupported CTSC event name '{}'", event.name), issues);
            continue;
        }
        let event_role = EventRole::parse(event.name.as_str());
        if event_role.operation_only() {
            require(
                span_role == SpanRole::Operation,
                &location,
                format!("{} must belong to a conformance.operation span", event.name),
                issues,
            );
        }
        match event_role {
            EventRole::Observation => {
                reject_attrs(&event.attributes, OBS_ATTRS, &location, issues);
                require_str(&event.attributes, "conformance.observation.name", path, &location, issues);
                require(
                    event.attributes.contains_key("conformance.observation.value"),
                    &location,
                    "missing conformance.observation.value",
                    issues,
                );
            }
            EventRole::Result => {
                reject_attrs(&event.attributes, RESULT_ATTRS, &location, issues);
                result_count += 1;
                terminated = true;
                require(
                    event.attributes.contains_key("conformance.result.value"),
                    &location,
                    "missing conformance.result.value",
                    issues,
                );
            }
            EventRole::Empty => {
                reject_attrs(&event.attributes, [], &location, issues);
                failure_count += 1;
                terminated = true;
            }
            EventRole::Error => {
                reject_attrs(&event.attributes, ERROR_ATTRS, &location, issues);
                failure_count += 1;
                terminated = true;
                require_str(&event.attributes, "conformance.error.name", path, &location, issues);
            }
            EventRole::Fault => {
                reject_attrs(&event.attributes, FAULT_ATTRS, &location, issues);
                failure_count += usize::from(span_role == SpanRole::Operation);
                terminated |= span_role == SpanRole::Operation;
                require(
                    matches!(span_role, SpanRole::Run | SpanRole::Scenario | SpanRole::Operation),
                    &location,
                    "fault must belong to a run, scenario, or operation span",
                    issues,
                );
                let fault_type = require_str(&event.attributes, "conformance.fault.type", path, &location, issues);
                let observer = require_str(&event.attributes, "conformance.fault.observer", path, &location, issues);
                if FaultObserver::parse(observer) == FaultObserver::Target {
                    require(
                        span_role == SpanRole::Operation,
                        &location,
                        "target fault must belong to an operation span",
                        issues,
                    );
                } else if FaultObserver::parse(observer) == FaultObserver::Supervisor {
                    require(
                        matches!(span_role, SpanRole::Run | SpanRole::Scenario),
                        &location,
                        "supervisor fault must belong to a run or scenario span",
                        issues,
                    );
                    if let Some(fault_type) = fault_type {
                        require(
                            SUPERVISOR_FAULTS.contains(&fault_type) || is_fault(fault_type),
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
                    check_attr(&event.attributes, key, AttributeType::String, &location, issues);
                }
                for key in ["conformance.fault.exit_code", "conformance.fault.timeout_ms"] {
                    check_attr(&event.attributes, key, AttributeType::Int, &location, issues);
                }
            }
            EventRole::Other => {}
        }
    }
    if span_role == SpanRole::Operation {
        require(
            failure_count <= 1,
            &span.location,
            "operation must not contain multiple non-result completion/failure events",
            issues,
        );
        require(
            result_count == 0 || failure_count == 0,
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
            .any(|event| matches!(EventRole::parse(event.name.as_str()), EventRole::Error | EventRole::Fault))
        {
            require(
                span.status_error,
                &span.location,
                "declared error and fault operations must have ERROR status",
                issues,
            );
        }
    }
    if matches!(span_role, SpanRole::Run | SpanRole::Scenario) && span.events.iter().any(|event| event.name == "conformance.fault") {
        require(
            span.status_error,
            &span.location,
            "fault-bearing run or scenario must have ERROR status",
            issues,
        );
    }
}
