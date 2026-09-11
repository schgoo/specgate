//! CTSC projection for `SpecGate` — translates legacy flat operation traces
//! into deterministic semantic CTSC format.
//!
//! `translate_legacy_trace` walks a JSON-encoded sequence of legacy
//! [`specgate_runtime::TraceEvent`]s — a leading `Run` event followed by
//! ordinary `Event`s — and re-projects it into a [`CtscProjection`]:
//!
//! - the leading `Run` event supplies `operation_name`;
//! - an event named `<operation_name>.<field>` is un-prefixed and becomes an
//!   operation input, keyed by `<field>`;
//! - every other ordinary event becomes an observation, keyed by its own name;
//! - the reserved `$result` / `$fault` event names select the terminal
//!   `completion` state (`"result"`, `"fault"`, or `"none"` if neither
//!   appears); that event's value becomes `completion_value_json`.
//!
//! Values keep their [`specgate_runtime::Value`] shape as-is: an event whose
//! value happens to look like `{"Integer": 7}` is a genuine single-entry map,
//! not a legacy tagged scalar, and is projected unchanged.
//!
//! `encode_legacy_trace_otlp` applies the same projection and emits one compact,
//! deterministic CTSC 0.1 OTLP JSON document containing a run span, its
//! scenario child, and one operation child. Caller-supplied identifiers,
//! timestamp, tool version, and target metadata make production identity
//! explicit while keeping tests reproducible.

use serde::{Deserialize, Serialize};
use specgate::{SpecEvent, spec_component, spec_operation};
use specgate_runtime::{TraceEvent, Value};
use std::collections::BTreeMap;

spec_component!("specgate.ctsc");

#[derive(Debug, Clone, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscProjection {
    #[spec_event]
    pub scenario_name: String,
    #[spec_event]
    pub component_id: String,
    #[spec_event]
    pub operation_name: String,
    #[spec_event]
    pub inputs_json: String,
    #[spec_event]
    pub observations_json: String,
    #[spec_event]
    pub completion: String,
    #[spec_event]
    pub completion_value_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscOtlpEncoding {
    #[spec_event]
    pub span_count: i32,
    #[spec_event]
    pub otlp_json: String,
}

/// The terminal legacy event, if any, that selects the CTSC completion state.
enum Completion {
    Result(Value),
    Fault(Value),
    None,
}

struct LegacyProjection {
    operation_name: String,
    inputs: BTreeMap<String, Value>,
    observations: Vec<(String, Value)>,
    completion: Completion,
}

#[spec_operation("translate_legacy_trace")]
pub fn translate_legacy_trace(scenario_name: String, component_id: String, legacy_trace_json: String) -> CtscProjection {
    let projection = project_legacy_trace(&legacy_trace_json);
    let mut observations_map: BTreeMap<String, Value> = BTreeMap::new();
    for (name, value) in projection.observations {
        observations_map.insert(name, value);
    }

    let (completion, completion_value_json) = completion_strings(projection.completion);

    CtscProjection {
        scenario_name,
        component_id,
        operation_name: projection.operation_name,
        inputs_json: to_json_string(&Value::Map(projection.inputs)),
        observations_json: to_json_string(&Value::Map(observations_map)),
        completion,
        completion_value_json,
    }
}

#[allow(clippy::too_many_arguments)]
#[spec_operation("encode_legacy_trace_otlp")]
pub fn encode_legacy_trace_otlp(
    scenario_name: String,
    component_id: String,
    legacy_trace_json: String,
    trace_id: String,
    run_span_id: String,
    scenario_span_id: String,
    operation_span_id: String,
    run_id: String,
    start_time_unix_nano: i64,
    tool_version: String,
    target_name: String,
    target_language: String,
) -> CtscOtlpEncoding {
    let projection = project_legacy_trace(&legacy_trace_json);
    let operation_has_fault = matches!(projection.completion, Completion::Fault(_));
    let status_code = if operation_has_fault { 2 } else { 1 };
    let mut operation_events = projection
        .observations
        .into_iter()
        .map(|(name, value)| {
            otlp_event(
                "conformance.observation",
                vec![
                    string_attribute("conformance.observation.name", name),
                    KeyValue {
                        key: "conformance.observation.value".to_string(),
                        value: value_to_any_value(&value),
                    },
                ],
            )
        })
        .collect::<Vec<_>>();

    match projection.completion {
        Completion::Result(value) => operation_events.push(otlp_event(
            "conformance.result",
            vec![KeyValue {
                key: "conformance.result.value".to_string(),
                value: value_to_any_value(&value),
            }],
        )),
        Completion::Fault(value) => {
            let mut attributes = vec![string_attribute("conformance.fault.type", "specgate.legacy_fault")];
            if let Value::String(message) = value {
                attributes.push(string_attribute("conformance.fault.message", message));
            }
            attributes.push(string_attribute("conformance.fault.observer", "target"));
            operation_events.push(otlp_event("conformance.fault", attributes));
        }
        Completion::None => {}
    }

    for (index, event) in operation_events.iter_mut().enumerate() {
        event.time_unix_nano = timestamp(start_time_unix_nano, 3 + i64::try_from(index).expect("event index must fit in i64"));
    }

    let operation_end_offset = 3 + i64::try_from(operation_events.len()).expect("event count must fit in i64");
    let schema_url = "https://specgate.dev/ctsc/schema/0.1.0".to_string();
    let document = OtlpDocument {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute("conformance.version", "0.1.0"),
                    string_attribute("conformance.tool.name", "specgate"),
                    string_attribute("conformance.tool.version", tool_version.clone()),
                    string_attribute("conformance.target.name", target_name),
                    string_attribute("conformance.target.language", target_language),
                ],
            },
            scope_spans: vec![ScopeSpans {
                scope: InstrumentationScope {
                    name: "specgate.ctsc",
                    version: tool_version,
                },
                spans: vec![
                    Span {
                        trace_id: trace_id.clone(),
                        id: run_span_id.clone(),
                        parent_id: None,
                        name: "conformance.run",
                        kind: 1,
                        start_time_unix_nano: timestamp(start_time_unix_nano, 0),
                        end_time_unix_nano: timestamp(start_time_unix_nano, operation_end_offset + 2),
                        attributes: vec![string_attribute("conformance.run.id", run_id)],
                        events: Vec::new(),
                        status: Status { code: 1 },
                    },
                    Span {
                        trace_id: trace_id.clone(),
                        id: scenario_span_id.clone(),
                        parent_id: Some(run_span_id),
                        name: "conformance.scenario",
                        kind: 1,
                        start_time_unix_nano: timestamp(start_time_unix_nano, 1),
                        end_time_unix_nano: timestamp(start_time_unix_nano, operation_end_offset + 1),
                        attributes: vec![
                            string_attribute("conformance.scenario.name", scenario_name),
                            integer_attribute("conformance.scenario.index", 0),
                        ],
                        events: Vec::new(),
                        status: Status { code: status_code },
                    },
                    Span {
                        trace_id,
                        id: operation_span_id,
                        parent_id: Some(scenario_span_id),
                        name: "conformance.operation",
                        kind: 1,
                        start_time_unix_nano: timestamp(start_time_unix_nano, 2),
                        end_time_unix_nano: timestamp(start_time_unix_nano, operation_end_offset),
                        attributes: vec![
                            string_attribute("conformance.component.id", component_id),
                            string_attribute("conformance.operation.name", projection.operation_name),
                            KeyValue {
                                key: "conformance.operation.inputs".to_string(),
                                value: AnyValue::Kvlist(KeyValueList {
                                    values: projection
                                        .inputs
                                        .into_iter()
                                        .map(|(key, value)| KeyValue {
                                            key,
                                            value: value_to_any_value(&value),
                                        })
                                        .collect(),
                                }),
                            },
                        ],
                        events: operation_events,
                        status: Status { code: status_code },
                    },
                ],
                schema_url: schema_url.clone(),
            }],
            schema_url,
        }],
    };

    let otlp_json = serde_json::to_string(&document).expect("OTLP document serialization must succeed");

    CtscOtlpEncoding { otlp_json, span_count: 3 }
}

fn project_legacy_trace(legacy_trace_json: &str) -> LegacyProjection {
    let trace_events: Vec<TraceEvent> = serde_json::from_str(legacy_trace_json).expect("valid trace JSON required");
    let mut operation_name = String::new();
    let mut events_iter = trace_events.iter().peekable();

    if let Some(TraceEvent::Run { operation }) = events_iter.peek() {
        operation_name.clone_from(operation);
        events_iter.next();
    }

    let mut inputs = BTreeMap::new();
    let mut observations = Vec::new();
    let mut completion = Completion::None;
    let input_prefix = format!("{operation_name}.");

    for event in events_iter {
        if let TraceEvent::Event { name, value } = event {
            if name == "$result" {
                completion = Completion::Result(value.clone());
            } else if name == "$fault" {
                completion = Completion::Fault(value.clone());
            } else if let Some(field_name) = name.strip_prefix(&input_prefix) {
                inputs.insert(field_name.to_string(), value.clone());
            } else {
                observations.push((name.clone(), value.clone()));
            }
        }
    }

    LegacyProjection {
        operation_name,
        inputs,
        observations,
        completion,
    }
}

fn completion_strings(completion: Completion) -> (String, String) {
    match completion {
        Completion::Result(value) => ("result".to_string(), to_json_string(&value)),
        Completion::Fault(value) => ("fault".to_string(), to_json_string(&value)),
        Completion::None => ("none".to_string(), String::new()),
    }
}

/// Serialize a Value to compact JSON string using its Serialize impl.
fn to_json_string(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

fn value_to_any_value(value: &Value) -> AnyValue {
    match value {
        Value::String(value) => AnyValue::String(value.clone()),
        Value::Integer(value) => AnyValue::Int(value.to_string()),
        Value::Float(value) if value.is_nan() => AnyValue::Double(DoubleValue::Symbol("NaN")),
        Value::Float(value) if value.is_infinite() && value.is_sign_positive() => AnyValue::Double(DoubleValue::Symbol("Infinity")),
        Value::Float(value) if value.is_infinite() => AnyValue::Double(DoubleValue::Symbol("-Infinity")),
        Value::Float(value) => AnyValue::Double(DoubleValue::Number(*value)),
        Value::Bool(value) => AnyValue::Bool(*value),
        Value::List(values) => AnyValue::Array(ArrayValue {
            values: values.iter().map(value_to_any_value).collect(),
        }),
        Value::Map(values) => AnyValue::Kvlist(KeyValueList {
            values: values
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone(),
                    value: value_to_any_value(value),
                })
                .collect(),
        }),
        Value::Set(values) => AnyValue::Array(ArrayValue {
            values: values.iter().map(value_to_any_value).collect(),
        }),
    }
}

fn string_attribute(key: &'static str, value: impl Into<String>) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: AnyValue::String(value.into()),
    }
}

fn integer_attribute(key: &'static str, value: i64) -> KeyValue {
    KeyValue {
        key: key.to_string(),
        value: AnyValue::Int(value.to_string()),
    }
}

fn otlp_event(name: &'static str, attributes: Vec<KeyValue>) -> SpanEvent {
    SpanEvent {
        time_unix_nano: String::new(),
        name,
        attributes,
    }
}

fn timestamp(start_time_unix_nano: i64, offset: i64) -> String {
    start_time_unix_nano.saturating_add(offset).to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OtlpDocument {
    resource_spans: Vec<ResourceSpans>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceSpans {
    resource: Resource,
    scope_spans: Vec<ScopeSpans>,
    schema_url: String,
}

#[derive(Serialize)]
struct Resource {
    attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScopeSpans {
    scope: InstrumentationScope,
    spans: Vec<Span>,
    schema_url: String,
}

#[derive(Serialize)]
struct InstrumentationScope {
    name: &'static str,
    version: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Span {
    trace_id: String,
    #[serde(rename = "spanId")]
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentSpanId")]
    parent_id: Option<String>,
    name: &'static str,
    kind: i32,
    start_time_unix_nano: String,
    end_time_unix_nano: String,
    attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    events: Vec<SpanEvent>,
    status: Status,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SpanEvent {
    time_unix_nano: String,
    name: &'static str,
    attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
struct Status {
    code: i32,
}

#[derive(Serialize)]
struct KeyValue {
    key: String,
    value: AnyValue,
}

#[derive(Serialize)]
enum AnyValue {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(DoubleValue),
    #[serde(rename = "arrayValue")]
    Array(ArrayValue),
    #[serde(rename = "kvlistValue")]
    Kvlist(KeyValueList),
}

#[derive(Serialize)]
#[serde(untagged)]
enum DoubleValue {
    Number(f64),
    Symbol(&'static str),
}

#[derive(Serialize)]
struct ArrayValue {
    values: Vec<AnyValue>,
}

#[derive(Serialize)]
struct KeyValueList {
    values: Vec<KeyValue>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn stateless_result() {
        let legacy_trace = r#"[{"kind":"Run","operation":"add"},{"kind":"Event","name":"add.a","value":2},{"kind":"Event","name":"add.b","value":3},{"kind":"Event","name":"$result","value":5}]"#;

        let result = translate_legacy_trace("add_2_3".to_string(), "fixture.stateless_add".to_string(), legacy_trace.to_string());

        assert_eq!(result.scenario_name, "add_2_3");
        assert_eq!(result.component_id, "fixture.stateless_add");
        assert_eq!(result.operation_name, "add");
        assert_eq!(result.inputs_json, r#"{"a":2,"b":3}"#);
        assert_eq!(result.observations_json, "{}");
        assert_eq!(result.completion, "result");
        assert_eq!(result.completion_value_json, "5");
    }

    #[test]
    fn observed_no_result() {
        let legacy_trace = r#"[{"kind":"Run","operation":"record_total"},{"kind":"Event","name":"record_total.amount","value":7},{"kind":"Event","name":"total","value":7}]"#;

        let result = translate_legacy_trace(
            "records_total".to_string(),
            "fixture.observation".to_string(),
            legacy_trace.to_string(),
        );

        assert_eq!(result.scenario_name, "records_total");
        assert_eq!(result.component_id, "fixture.observation");
        assert_eq!(result.operation_name, "record_total");
        assert_eq!(result.inputs_json, r#"{"amount":7}"#);
        assert_eq!(result.observations_json, r#"{"total":7}"#);
        assert_eq!(result.completion, "none");
        assert_eq!(result.completion_value_json, "");
    }

    #[test]
    fn unexpected_fault() {
        let legacy_trace = r#"[{"kind":"Run","operation":"explode"},{"kind":"Event","name":"explode.code","value":9},{"kind":"Event","name":"$fault","value":"boom"}]"#;

        let result = translate_legacy_trace("crashes".to_string(), "fixture.fault".to_string(), legacy_trace.to_string());

        assert_eq!(result.scenario_name, "crashes");
        assert_eq!(result.component_id, "fixture.fault");
        assert_eq!(result.operation_name, "explode");
        assert_eq!(result.inputs_json, r#"{"code":9}"#);
        assert_eq!(result.observations_json, "{}");
        assert_eq!(result.completion, "fault");
        assert_eq!(result.completion_value_json, r#""boom""#);
    }

    #[test]
    fn preserve_single_entry_map() {
        let legacy_trace = r#"[{"kind":"Run","operation":"record_metric"},{"kind":"Event","name":"metric","value":{"Integer":7}}]"#;

        let result = translate_legacy_trace(
            "records_metric".to_string(),
            "fixture.observation".to_string(),
            legacy_trace.to_string(),
        );

        assert_eq!(result.scenario_name, "records_metric");
        assert_eq!(result.component_id, "fixture.observation");
        assert_eq!(result.operation_name, "record_metric");
        assert_eq!(result.inputs_json, "{}");
        assert_eq!(result.observations_json, r#"{"metric":{"Integer":7}}"#);
        assert_eq!(result.completion, "none");
        assert_eq!(result.completion_value_json, "");
    }

    #[test]
    fn encode_stateless_result_as_otlp() {
        let legacy_trace = r#"[{"kind":"Run","operation":"add"},{"kind":"Event","name":"add.a","value":2},{"kind":"Event","name":"add.b","value":3},{"kind":"Event","name":"$result","value":5}]"#;

        let result = encode_legacy_trace_otlp(
            "add_2_3".to_string(),
            "fixture.stateless_add".to_string(),
            legacy_trace.to_string(),
            "11111111111111111111111111111111".to_string(),
            "1111111111111101".to_string(),
            "1111111111111102".to_string(),
            "1111111111111103".to_string(),
            "run-001".to_string(),
            1_000_000_000,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
        );

        assert_eq!(result.span_count, 3);
        assert!(!result.otlp_json.contains('\n'));
        assert!(!result.otlp_json.contains(": "));
        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let resource_spans = &document["resourceSpans"][0];
        let resource_attributes = resource_spans["resource"]["attributes"].as_array().unwrap();
        let spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap();

        assert_eq!(resource_spans["schemaUrl"], "https://specgate.dev/ctsc/schema/0.1.0");
        assert_eq!(resource_attributes.len(), 5);
        assert_eq!(resource_attributes[0]["key"], "conformance.version");
        assert_eq!(resource_attributes[0]["value"]["stringValue"], "0.1.0");
        assert_eq!(resource_attributes[1]["key"], "conformance.tool.name");
        assert_eq!(resource_attributes[1]["value"]["stringValue"], "specgate");
        assert_eq!(resource_attributes[2]["key"], "conformance.tool.version");
        assert_eq!(resource_attributes[2]["value"]["stringValue"], "0.5.0");
        assert_eq!(resource_attributes[3]["key"], "conformance.target.name");
        assert_eq!(resource_attributes[3]["value"]["stringValue"], "rust-reference");
        assert_eq!(resource_attributes[4]["key"], "conformance.target.language");
        assert_eq!(resource_attributes[4]["value"]["stringValue"], "rust");
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0]["name"], "conformance.run");
        assert_eq!(spans.iter().filter(|span| span["name"] == "conformance.run").count(), 1);
        assert_eq!(spans.iter().filter(|span| span["name"] == "conformance.scenario").count(), 1);
        assert_eq!(spans.iter().filter(|span| span["name"] == "conformance.operation").count(), 1);
        assert_eq!(spans[0]["startTimeUnixNano"], "1000000000");
        assert_eq!(spans[0]["endTimeUnixNano"], "1000000006");
        assert_eq!(spans[1]["parentSpanId"], "1111111111111101");
        assert_eq!(spans[1]["startTimeUnixNano"], "1000000001");
        assert_eq!(spans[1]["endTimeUnixNano"], "1000000005");
        assert_eq!(spans[2]["parentSpanId"], "1111111111111102");
        assert_eq!(spans[2]["startTimeUnixNano"], "1000000002");
        assert_eq!(spans[2]["endTimeUnixNano"], "1000000004");
        assert_eq!(
            spans[2]["attributes"][2]["value"]["kvlistValue"]["values"][0]["value"]["intValue"],
            "2"
        );
        assert_eq!(spans[2]["events"][0]["timeUnixNano"], "1000000003");
        assert_eq!(spans[2]["events"][0]["attributes"][0]["value"]["intValue"], "5");
    }

    #[test]
    fn encode_operation_emits_spec_outputs_in_case_order() {
        specgate_runtime::reset();
        let result = encode_test_trace(r#"[{"kind":"Run","operation":"add"},{"kind":"Event","name":"$result","value":5}]"#);
        let traces = specgate_runtime::take_traces();
        let output_events = &traces[traces.len() - 3..];

        assert!(matches!(
            &output_events[0],
            TraceEvent::Event { name, value: Value::Integer(3) } if name == "span_count"
        ));
        assert!(matches!(
            &output_events[1],
            TraceEvent::Event { name, value: Value::String(value) }
                if name == "otlp_json" && value == &result.otlp_json
        ));
        assert!(matches!(
            &output_events[2],
            TraceEvent::Event { name, value: Value::Map(_) } if name == "$result"
        ));
    }

    #[test]
    fn observations_preserve_legacy_order() {
        let legacy_trace =
            r#"[{"kind":"Run","operation":"record"},{"kind":"Event","name":"z","value":1},{"kind":"Event","name":"a","value":true}]"#;

        let result = encode_test_trace(legacy_trace);
        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let events = document["resourceSpans"][0]["scopeSpans"][0]["spans"][2]["events"]
            .as_array()
            .unwrap();

        assert_eq!(events[0]["name"], "conformance.observation");
        assert_eq!(events[0]["attributes"][0]["value"]["stringValue"], "z");
        assert_eq!(events[0]["attributes"][1]["value"]["intValue"], "1");
        assert_eq!(events[1]["attributes"][0]["value"]["stringValue"], "a");
        assert_eq!(events[1]["attributes"][1]["value"]["boolValue"], true);
        assert_eq!(document["resourceSpans"][0]["scopeSpans"][0]["spans"][2]["status"]["code"], 1);
    }

    #[test]
    fn no_result_emits_no_terminal_event() {
        let result = encode_test_trace(r#"[{"kind":"Run","operation":"noop"}]"#);
        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let operation = &document["resourceSpans"][0]["scopeSpans"][0]["spans"][2];

        assert!(operation.get("events").is_none());
        assert_eq!(operation["status"]["code"], 1);
    }

    #[test]
    fn faults_use_minimal_deterministic_legacy_mapping() {
        let legacy_trace = r#"[{"kind":"Run","operation":"explode"},{"kind":"Event","name":"$fault","value":"boom"}]"#;

        let result = encode_test_trace(legacy_trace);
        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap();
        let fault = &spans[2]["events"][0];

        assert_eq!(spans[0]["status"]["code"], 1);
        assert_eq!(spans[1]["status"]["code"], 2);
        assert_eq!(spans[2]["status"]["code"], 2);
        assert_eq!(fault["name"], "conformance.fault");
        assert_eq!(fault["attributes"][0]["value"]["stringValue"], "specgate.legacy_fault");
        assert_eq!(fault["attributes"][1]["value"]["stringValue"], "boom");
        assert_eq!(fault["attributes"][2]["value"]["stringValue"], "target");
    }

    #[test]
    fn recursively_encodes_values_with_stable_collection_order() {
        let mut nested_map = BTreeMap::new();
        nested_map.insert("z".to_string(), Value::Float(2.5));
        nested_map.insert("a".to_string(), Value::List(vec![Value::Bool(true), Value::Integer(-7)]));

        let mut set = BTreeSet::new();
        set.insert(Value::String("zeta".to_string()));
        set.insert(Value::Integer(4));
        set.insert(Value::Bool(false));

        let mut root = BTreeMap::new();
        root.insert("set".to_string(), Value::Set(set));
        root.insert("map".to_string(), Value::Map(nested_map));

        let encoded = serde_json::to_string(&value_to_any_value(&Value::Map(root))).unwrap();

        assert_eq!(
            encoded,
            r#"{"kvlistValue":{"values":[{"key":"map","value":{"kvlistValue":{"values":[{"key":"a","value":{"arrayValue":{"values":[{"boolValue":true},{"intValue":"-7"}]}}},{"key":"z","value":{"doubleValue":2.5}}]}}},{"key":"set","value":{"arrayValue":{"values":[{"boolValue":false},{"intValue":"4"},{"stringValue":"zeta"}]}}}]}}"#
        );
    }

    #[test]
    fn encodes_symbolic_non_finite_floats() {
        let values = Value::List(vec![
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Float(f64::NEG_INFINITY),
        ]);

        let encoded = serde_json::to_string(&value_to_any_value(&values)).unwrap();

        assert_eq!(
            encoded,
            r#"{"arrayValue":{"values":[{"doubleValue":"NaN"},{"doubleValue":"Infinity"},{"doubleValue":"-Infinity"}]}}"#
        );
    }

    fn encode_test_trace(legacy_trace: &str) -> CtscOtlpEncoding {
        encode_legacy_trace_otlp(
            "scenario".to_string(),
            "fixture.component".to_string(),
            legacy_trace.to_string(),
            "11111111111111111111111111111111".to_string(),
            "1111111111111101".to_string(),
            "1111111111111102".to_string(),
            "1111111111111103".to_string(),
            "run-001".to_string(),
            1_000_000_000,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
        )
    }
}
