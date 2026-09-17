//! CTSC projection for `SpecGate` — encodes native structured operation
//! capture and provides transitional translation for legacy flat traces.
//!
//! Native synchronous Rust capture now creates operation spans at real
//! `#[spec_operation]` invocation boundaries, including nested parentage,
//! typed inputs and results, observations, logical timestamps, and status. Its
//! public producer operations expose CTSC artifacts only.
//! Native input/result projection is type-aware and recursively preserves CTSC
//! 0.2 option wrappers inside supported collections and annotated records.
//! Ordered native sidecars can be merged into one deterministic run with
//! registry identity, version, and digest resource attributes for Linked
//! validation.
//! Capture bundles can also be decoded through [`decode_replay_bundle_result`],
//! which verifies manifest format/version, exact registry and trace digests,
//! trace registry linkage, scenario order, operation declarations, and typed
//! primitive inputs before returning ordered [`ReplayScenario`] values. The
//! decoder exposes only top-level operations as replay instructions. Candidate
//! native captures use [`encode_replayed_native_captures_otlp_result`] to retain
//! the original registry identity while using deterministic IDs independent
//! from the reference run.
//!
//! The legacy `Run`/`Event` buffer and the translation operations below remain
//! temporarily for extraction and harness subsystems that do not yet have CTSC
//! replacements. They are not a stable compatibility surface.
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
//! deterministic CTSC 0.2 OTLP JSON document containing a run span, its
//! scenario child, and one operation child. Caller-supplied identifiers,
//! timestamp, tool version, and target metadata make production identity
//! explicit while keeping tests reproducible.
//!
//! `encode_discovery_registry` preserves the original raw-discovery projection
//! for primitive operations. `encode_schema_registry` accepts the harness's
//! normalized, setup-folded schema and emits named records, tagged unions, and
//! recursive CTSC collection/option references without reimplementing
//! language-specific discovery or normalization.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specgate::{SpecEvent, spec_component, spec_operation};
use specgate_runtime::{
    NativeCapture, NativeCaptureConfig, NativeCompletion, NativeOperationSpan, NativeStatus, TraceEvent, Value, finish_native_capture,
    start_native_capture,
};
use std::collections::{BTreeMap, BTreeSet};

spec_component!("specgate.ctsc");

const CTSC_VERSION: &str = "0.2.0";
const CTSC_SCHEMA_URL: &str = "https://specgate.dev/ctsc/schema/0.2.0";

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscNativeCaptureEncoding {
    #[spec_event]
    pub span_count: i32,
    #[spec_event]
    pub otlp_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscNativeOptionalCaptureEncoding {
    #[spec_event]
    pub otlp_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscRegistryEncoding {
    #[spec_event]
    pub operation_count: i32,
    #[spec_event]
    pub type_count: i32,
    #[spec_event]
    pub registry_json: String,
}

/// A registry type used by typed replay decoding and semantic linking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReplayType {
    Primitive { name: String },
    Named { name: String },
    List { items: Box<ReplayType> },
    Set { items: Box<ReplayType> },
    Map { keys: Box<ReplayType>, values: Box<ReplayType> },
    Tuple { items: Vec<ReplayType> },
    Optional { value: Box<ReplayType> },
    Record { fields: Vec<ReplayRegistryInput> },
}

impl ReplayType {
    /// Return the primitive name when this is a primitive type.
    #[must_use]
    pub fn primitive_name(&self) -> Option<&str> {
        match self {
            Self::Primitive { name } => Some(name),
            _ => None,
        }
    }
}

/// One named value declaration in a replay registry operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistryInput {
    pub name: String,
    #[serde(rename = "type")]
    pub value_type: ReplayType,
}

/// One linked operation declaration needed by the replay planner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistryOperation {
    pub component_id: String,
    pub name: String,
    pub inputs: Vec<ReplayRegistryInput>,
    pub output: Option<ReplayType>,
}

/// Registry identity and operation declarations verified from a capture bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayRegistry {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub operations: Vec<ReplayRegistryOperation>,
}

/// A losslessly decoded CTSC value supported by the first replay slice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ReplayValue {
    Unit,
    String(String),
    Bool(bool),
    I32(i32),
    I64(i64),
    U32(u32),
    U64(u64),
    F32Bits(u32),
    F64Bits(u64),
}

/// One named semantic input decoded according to its linked registry type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayInput {
    pub name: String,
    pub value_type: ReplayType,
    pub value: ReplayValue,
}

/// One top-level reference operation that must be invoked by replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayOperation {
    pub component_id: String,
    pub operation_name: String,
    pub inputs: Vec<ReplayInput>,
    pub output: Option<ReplayType>,
}

/// One ordered reference scenario and its sequential top-level operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayScenario {
    pub name: String,
    pub index: i64,
    pub operations: Vec<ReplayOperation>,
}

/// A manifest-, digest-, registry-, and trace-linked replay bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayBundle {
    pub component_id: String,
    pub registry: ReplayRegistry,
    pub scenarios: Vec<ReplayScenario>,
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
    let schema_url = CTSC_SCHEMA_URL.to_string();
    let document = OtlpDocument {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute("conformance.version", CTSC_VERSION),
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

#[spec_operation("double", spec = "fixture.native_capture")]
fn double(value: i32) -> i32 {
    value * 2
}

#[spec_operation("add_after_double", spec = "fixture.native_capture")]
fn add_after_double(value: i32) -> i32 {
    double(value + 1) + 1
}

#[spec_operation("echo_optional", spec = "fixture.native_capture")]
fn echo_optional(value: Option<String>) -> Option<String> {
    value
}

#[allow(clippy::too_many_arguments)]
#[spec_operation("capture_native_rust_otlp")]
pub fn capture_native_rust_otlp(
    scenario_name: String,
    trace_id: String,
    run_span_id: String,
    scenario_span_id: String,
    operation_span_ids_json: String,
    run_id: String,
    start_time_unix_nano: i64,
    clock_step_unix_nano: i64,
    tool_version: String,
    target_name: String,
    target_language: String,
    value: i32,
) -> CtscNativeCaptureEncoding {
    let operation_span_ids = serde_json::from_str::<Vec<String>>(&operation_span_ids_json)
        .unwrap_or_else(|error| panic!("malformed operation span ID JSON: {error}"));
    start_native_capture(NativeCaptureConfig {
        scenario_name,
        trace_id,
        run_span_id,
        scenario_span_id,
        operation_span_ids,
        start_time_unix_nano,
        clock_step_unix_nano,
    })
    .unwrap_or_else(|error| panic!("failed to start native capture: {error}"));

    let result = add_after_double(value);
    debug_assert_eq!(result, 7, "native fixture must preserve its specified result");
    let capture = finish_native_capture().unwrap_or_else(|error| panic!("failed to finish native capture: {error}"));
    let otlp_json = encode_native_capture(&capture, run_id, tool_version, target_name, target_language);
    let span_count = i32::try_from(capture.operations.len() + 2).unwrap_or_else(|_error| panic!("native capture span count exceeds i32"));

    CtscNativeCaptureEncoding { span_count, otlp_json }
}

#[allow(clippy::too_many_arguments)]
#[spec_operation("capture_native_optional_otlp")]
pub fn capture_native_optional_otlp(
    scenario_name: String,
    trace_id: String,
    run_span_id: String,
    scenario_span_id: String,
    operation_span_id: String,
    run_id: String,
    start_time_unix_nano: i64,
    clock_step_unix_nano: i64,
    tool_version: String,
    target_name: String,
    target_language: String,
    present: bool,
    value: String,
) -> CtscNativeOptionalCaptureEncoding {
    start_native_capture(NativeCaptureConfig {
        scenario_name,
        trace_id,
        run_span_id,
        scenario_span_id,
        operation_span_ids: vec![operation_span_id],
        start_time_unix_nano,
        clock_step_unix_nano,
    })
    .unwrap_or_else(|error| panic!("failed to start native optional capture: {error}"));

    let input = present.then_some(value);
    let result = echo_optional(input.clone());
    debug_assert_eq!(result, input, "optional fixture must echo its input");
    let capture = finish_native_capture().unwrap_or_else(|error| panic!("failed to finish native optional capture: {error}"));
    let otlp_json = encode_native_capture(&capture, run_id, tool_version, target_name, target_language);

    CtscNativeOptionalCaptureEncoding { otlp_json }
}

fn encode_native_capture(
    capture: &NativeCapture,
    run_id: String,
    tool_version: String,
    target_name: String,
    target_language: String,
) -> String {
    let run_status_code = status_code(capture.run.status);
    let schema_url = CTSC_SCHEMA_URL.to_string();
    let mut spans = vec![
        Span {
            trace_id: capture.trace_id.clone(),
            id: capture.run.span_id.clone(),
            parent_id: None,
            name: "conformance.run",
            kind: 1,
            start_time_unix_nano: capture.run.start_time_unix_nano.to_string(),
            end_time_unix_nano: capture.run.end_time_unix_nano.to_string(),
            attributes: vec![string_attribute("conformance.run.id", run_id)],
            events: Vec::new(),
            status: Status { code: run_status_code },
        },
        Span {
            trace_id: capture.trace_id.clone(),
            id: capture.scenario.span_id.clone(),
            parent_id: capture.scenario.parent_span_id.clone(),
            name: "conformance.scenario",
            kind: 1,
            start_time_unix_nano: capture.scenario.start_time_unix_nano.to_string(),
            end_time_unix_nano: capture.scenario.end_time_unix_nano.to_string(),
            attributes: vec![
                string_attribute("conformance.scenario.name", capture.scenario_name.clone()),
                integer_attribute("conformance.scenario.index", 0),
            ],
            events: Vec::new(),
            status: Status {
                code: status_code(capture.scenario.status),
            },
        },
    ];
    spans.extend(
        capture
            .operations
            .iter()
            .map(|operation| native_operation_span(&capture.trace_id, operation)),
    );

    let document = OtlpDocument {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute("conformance.version", CTSC_VERSION),
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
                spans,
                schema_url: schema_url.clone(),
            }],
            schema_url,
        }],
    };
    serde_json::to_string(&document).expect("native OTLP document serialization must succeed")
}

/// Merge ordered native scenario captures into one deterministic linked CTSC
/// run.
///
/// Capture-local identifiers and logical timestamps are rebased into one
/// non-zero trace/span sequence. Operation parentage, semantic inputs,
/// observations, completions, and scenario order are preserved.
///
/// # Errors
///
/// Returns an error for an empty capture list, malformed native parentage,
/// identifier exhaustion, timestamp overflow, or JSON serialization failure.
#[allow(clippy::too_many_arguments)]
pub fn encode_native_captures_otlp_result(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    encode_native_captures_with_identity(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        "00000000000000000000000000000001",
        1,
        "specgate.capture",
    )
}

/// Encode candidate replay captures as one deterministic run with identity
/// independent from the reference capture encoder.
///
/// # Errors
///
/// Returns the same structural, identifier, timestamp, and serialization
/// errors as [`encode_native_captures_otlp_result`].
#[allow(clippy::too_many_arguments)]
pub fn encode_replayed_native_captures_otlp_result(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    encode_native_captures_with_identity(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        "00000000000000000000000000000002",
        0x8000_0000_0000_0001,
        "specgate.replay",
    )
}

#[allow(clippy::too_many_arguments)]
fn encode_native_captures_with_identity(
    captures: &[NativeCapture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
    trace_id: &str,
    first_span_id: u64,
    run_id: &str,
) -> Result<CtscNativeCaptureEncoding, String> {
    if captures.is_empty() {
        return Err("cannot encode a CTSC run without captured scenarios".to_string());
    }

    let trace_id = trace_id.to_string();
    let run_span_id = deterministic_span_id(first_span_id)?;
    let mut next_span_id = first_span_id
        .checked_add(1)
        .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
    let mut next_scenario_time = 2_i64;
    let mut scenario_spans = Vec::new();
    let mut operation_count = 0_usize;
    let mut run_has_error = false;

    for (scenario_index, capture) in captures.iter().enumerate() {
        let scenario_span_id = deterministic_span_id(next_span_id)?;
        next_span_id = next_span_id
            .checked_add(1)
            .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;

        let mut operation_ids = BTreeMap::new();
        for operation in &capture.operations {
            let span_id = deterministic_span_id(next_span_id)?;
            next_span_id = next_span_id
                .checked_add(1)
                .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
            if operation_ids.insert(operation.span_id.clone(), span_id).is_some() {
                return Err(format!(
                    "native scenario '{}' contains duplicate operation span ID '{}'",
                    capture.scenario_name, operation.span_id
                ));
            }
        }

        let time_offset = next_scenario_time
            .checked_sub(capture.scenario.start_time_unix_nano)
            .ok_or_else(|| "CTSC scenario timestamp offset overflow".to_string())?;
        let scenario_end_time = offset_time(capture.scenario.end_time_unix_nano, time_offset)?;
        let scenario_index = i64::try_from(scenario_index).map_err(|_error| "CTSC scenario index exceeds i64".to_string())?;
        run_has_error |= capture.scenario.status == NativeStatus::Error;
        scenario_spans.push(Span {
            trace_id: trace_id.clone(),
            id: scenario_span_id.clone(),
            parent_id: Some(run_span_id.clone()),
            name: "conformance.scenario",
            kind: 1,
            start_time_unix_nano: next_scenario_time.to_string(),
            end_time_unix_nano: scenario_end_time.to_string(),
            attributes: vec![
                string_attribute("conformance.scenario.name", capture.scenario_name.clone()),
                integer_attribute("conformance.scenario.index", scenario_index),
            ],
            events: Vec::new(),
            status: Status {
                code: status_code(capture.scenario.status),
            },
        });

        for operation in &capture.operations {
            let span_id = operation_ids
                .get(&operation.span_id)
                .cloned()
                .ok_or_else(|| "native operation span ID mapping was not created".to_string())?;
            let parent_span_id = if operation.parent_span_id == capture.scenario.span_id {
                scenario_span_id.clone()
            } else {
                operation_ids.get(&operation.parent_span_id).cloned().ok_or_else(|| {
                    format!(
                        "native scenario '{}' operation '{}' has unresolved parent span '{}'",
                        capture.scenario_name, operation.operation_name, operation.parent_span_id
                    )
                })?
            };
            let rebased = rebase_native_operation(operation, span_id, parent_span_id, time_offset)?;
            scenario_spans.push(native_operation_span(&trace_id, &rebased));
            operation_count = operation_count
                .checked_add(1)
                .ok_or_else(|| "CTSC operation count overflow".to_string())?;
        }

        next_scenario_time = scenario_end_time
            .checked_add(1)
            .ok_or_else(|| "CTSC logical timestamp overflow".to_string())?;
    }

    let run_end_time = next_scenario_time;
    let mut spans = vec![Span {
        trace_id: trace_id.clone(),
        id: run_span_id,
        parent_id: None,
        name: "conformance.run",
        kind: 1,
        start_time_unix_nano: "1".to_string(),
        end_time_unix_nano: run_end_time.to_string(),
        attributes: vec![string_attribute("conformance.run.id", run_id)],
        events: Vec::new(),
        status: Status {
            code: status_code(if run_has_error { NativeStatus::Error } else { NativeStatus::Ok }),
        },
    }];
    spans.extend(scenario_spans);

    let schema_url = CTSC_SCHEMA_URL.to_string();
    let document = OtlpDocument {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute("conformance.version", CTSC_VERSION),
                    string_attribute("conformance.tool.name", "specgate"),
                    string_attribute("conformance.tool.version", tool_version),
                    string_attribute("conformance.target.name", target_name),
                    string_attribute("conformance.target.language", target_language),
                    string_attribute("conformance.registry.id", registry_id),
                    string_attribute("conformance.registry.version", registry_version),
                    string_attribute("conformance.registry.digest", registry_digest),
                ],
            },
            scope_spans: vec![ScopeSpans {
                scope: InstrumentationScope {
                    name: "specgate.ctsc",
                    version: tool_version.to_string(),
                },
                spans,
                schema_url: schema_url.clone(),
            }],
            schema_url,
        }],
    };
    let otlp_json = serde_json::to_string(&document).map_err(|error| format!("native CTSC OTLP serialization failed: {error}"))?;
    let span_count =
        i32::try_from(operation_count + captures.len() + 1).map_err(|_error| "native CTSC span count exceeds i32".to_string())?;
    Ok(CtscNativeCaptureEncoding { span_count, otlp_json })
}

/// Decode and link-validate the three exact files in a capture bundle.
///
/// The manifest format/version, exact registry and trace digests, registry
/// identity/version/digest trace attributes, span parentage, scenario order,
/// operation declarations, input names, and supported typed values are checked
/// before any replay scenario is returned. Only top-level operation spans are
/// returned as replay instructions; nested operations are validated but remain
/// observed reference behavior.
///
/// # Errors
///
/// Returns an actionable error for malformed JSON, invalid manifests or
/// digests, broken trace linkage, empty scenarios, or unsupported values.
pub fn decode_replay_bundle_result(manifest_json: &[u8], registry_json: &[u8], reference_otlp_json: &[u8]) -> Result<ReplayBundle, String> {
    let manifest: CaptureManifestWire =
        serde_json::from_slice(manifest_json).map_err(|error| format!("malformed capture manifest JSON: {error}"))?;
    validate_capture_manifest(&manifest)?;

    let registry_digest = replay_sha256_digest(registry_json);
    if manifest.registry.digest != registry_digest {
        return Err(format!(
            "capture registry digest mismatch: manifest declares '{}' but exact registry bytes digest to '{registry_digest}'",
            manifest.registry.digest
        ));
    }
    let reference_digest = replay_sha256_digest(reference_otlp_json);
    if manifest.reference.digest != reference_digest {
        return Err(format!(
            "capture reference digest mismatch: manifest declares '{}' but exact reference bytes digest to '{reference_digest}'",
            manifest.reference.digest
        ));
    }

    let registry_document: ReplayRegistryDocumentWire =
        serde_json::from_slice(registry_json).map_err(|error| format!("malformed CTSC registry JSON: {error}"))?;
    if registry_document.format != "ctsc.registry" || registry_document.format_version != CTSC_VERSION {
        return Err(format!(
            "unsupported CTSC registry format/version '{}/{}'; expected 'ctsc.registry/{CTSC_VERSION}'",
            registry_document.format, registry_document.format_version
        ));
    }
    if registry_document.registry_id != manifest.registry.id {
        return Err(format!(
            "capture manifest registry ID '{}' does not match registry document '{}'",
            manifest.registry.id, registry_document.registry_id
        ));
    }
    if registry_document.version != manifest.registry.version {
        return Err(format!(
            "capture manifest registry version '{}' does not match registry document '{}'",
            manifest.registry.version, registry_document.version
        ));
    }

    let registry = replay_registry(&registry_document, registry_digest)?;
    if !registry
        .operations
        .iter()
        .any(|operation| operation.component_id == manifest.component_id)
    {
        return Err(format!(
            "capture component '{}' is absent from registry '{}'",
            manifest.component_id, registry.id
        ));
    }

    let document: ReplayOtlpDocumentWire =
        serde_json::from_slice(reference_otlp_json).map_err(|error| format!("malformed reference OTLP JSON: {error}"))?;
    let scenarios = decode_replay_scenarios(&document, &manifest, &registry)?;

    Ok(ReplayBundle {
        component_id: manifest.component_id,
        registry,
        scenarios,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CaptureManifestWire {
    format: String,
    format_version: String,
    component_id: String,
    target: CaptureManifestTargetWire,
    tool: CaptureManifestToolWire,
    registry: CaptureManifestRegistryWire,
    reference: CaptureManifestReferenceWire,
    scenarios: CaptureManifestScenariosWire,
}

#[derive(Deserialize)]
struct CaptureManifestTargetWire {
    name: String,
    language: String,
}

#[derive(Deserialize)]
struct CaptureManifestToolWire {
    name: String,
    version: String,
}

#[derive(Deserialize)]
struct CaptureManifestRegistryWire {
    path: String,
    id: String,
    version: String,
    digest: String,
}

#[derive(Deserialize)]
struct CaptureManifestReferenceWire {
    path: String,
    digest: String,
}

#[derive(Deserialize)]
struct CaptureManifestScenariosWire {
    count: i32,
    names: Vec<String>,
}

fn validate_capture_manifest(manifest: &CaptureManifestWire) -> Result<(), String> {
    if manifest.format != "specgate.capture-manifest" || manifest.format_version != "0.1.0" {
        return Err(format!(
            "unsupported capture manifest format/version '{}/{}'; expected 'specgate.capture-manifest/0.1.0'",
            manifest.format, manifest.format_version
        ));
    }
    if manifest.registry.path != "registry.ctsc.json" {
        return Err(format!(
            "capture manifest registry path must be 'registry.ctsc.json', found '{}'",
            manifest.registry.path
        ));
    }
    if manifest.reference.path != "reference.otlp.json" {
        return Err(format!(
            "capture manifest reference path must be 'reference.otlp.json', found '{}'",
            manifest.reference.path
        ));
    }
    if manifest.component_id.is_empty()
        || manifest.target.name.is_empty()
        || manifest.target.language.is_empty()
        || manifest.tool.name.is_empty()
        || manifest.tool.version.is_empty()
        || manifest.registry.id.is_empty()
        || manifest.registry.version.is_empty()
    {
        return Err("capture manifest identity fields must not be empty".to_string());
    }
    validate_replay_digest("manifest registry", &manifest.registry.digest)?;
    validate_replay_digest("manifest reference", &manifest.reference.digest)?;
    let scenario_count =
        i32::try_from(manifest.scenarios.names.len()).map_err(|_error| "capture manifest scenario count exceeds i32".to_string())?;
    if manifest.scenarios.count != scenario_count {
        return Err(format!(
            "capture manifest scenario count {} does not match {} scenario names",
            manifest.scenarios.count, scenario_count
        ));
    }
    if manifest.scenarios.count <= 0 {
        return Err("capture manifest must declare at least one scenario".to_string());
    }
    let unique_names = manifest.scenarios.names.iter().collect::<BTreeSet<_>>();
    if unique_names.len() != manifest.scenarios.names.len() {
        return Err("capture manifest contains duplicate scenario names".to_string());
    }
    Ok(())
}

fn validate_replay_digest(label: &str, digest: &str) -> Result<(), String> {
    let valid = digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
    if valid {
        Ok(())
    } else {
        Err(format!(
            "{label} digest must be 'sha256:' followed by 64 lowercase hexadecimal characters"
        ))
    }
}

fn replay_sha256_digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayRegistryDocumentWire {
    format: String,
    format_version: String,
    registry_id: String,
    version: String,
    components: Vec<RegistryComponent>,
}

fn replay_registry(document: &ReplayRegistryDocumentWire, digest: String) -> Result<ReplayRegistry, String> {
    let mut component_ids = BTreeSet::new();
    let mut operation_keys = BTreeSet::new();
    let mut operations = Vec::new();
    for component in &document.components {
        if component.id.is_empty() || !component_ids.insert(component.id.as_str()) {
            return Err(format!(
                "CTSC registry contains an empty or duplicate component ID '{}'",
                component.id
            ));
        }
        for operation in &component.operations {
            if operation.name.is_empty() || !operation_keys.insert((component.id.as_str(), operation.name.as_str())) {
                return Err(format!(
                    "CTSC registry contains an empty or duplicate operation '{}::{}'",
                    component.id, operation.name
                ));
            }
            let input_names = operation.inputs.iter().map(|input| input.name.as_str()).collect::<BTreeSet<_>>();
            if input_names.len() != operation.inputs.len() {
                return Err(format!(
                    "CTSC registry operation '{}::{}' contains duplicate input names",
                    component.id, operation.name
                ));
            }
            operations.push(ReplayRegistryOperation {
                component_id: component.id.clone(),
                name: operation.name.clone(),
                inputs: operation.inputs.clone(),
                output: operation.outcomes.result.clone(),
            });
        }
    }
    if operations.is_empty() {
        return Err("CTSC registry contains no operations".to_string());
    }
    Ok(ReplayRegistry {
        id: document.registry_id.clone(),
        version: document.version.clone(),
        digest,
        operations,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayOtlpDocumentWire {
    resource_spans: Vec<ReplayResourceSpansWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplayResourceSpansWire {
    resource: ReplayResourceWire,
    scope_spans: Vec<ReplayScopeSpansWire>,
}

#[derive(Deserialize)]
struct ReplayResourceWire {
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
}

#[derive(Deserialize)]
struct ReplayScopeSpansWire {
    #[serde(default)]
    spans: Vec<ReplaySpanWire>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReplaySpanWire {
    trace_id: String,
    #[serde(rename = "spanId")]
    span_id: String,
    #[serde(default, rename = "parentSpanId")]
    parent_span_id: String,
    name: String,
    start_time_unix_nano: String,
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
    #[serde(default)]
    events: Vec<ReplayEventWire>,
}

#[derive(Deserialize)]
struct ReplayEventWire {
    name: String,
    #[serde(default)]
    attributes: Vec<ReplayKeyValueWire>,
}

#[derive(Deserialize)]
struct ReplayKeyValueWire {
    key: String,
    value: ReplayAnyValueWire,
}

#[allow(dead_code)]
#[derive(Deserialize)]
enum ReplayAnyValueWire {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(ReplayDoubleWire),
    #[serde(rename = "bytesValue")]
    Bytes(String),
    #[serde(rename = "arrayValue")]
    Array(ReplayArrayWire),
    #[serde(rename = "kvlistValue")]
    Kvlist(ReplayKeyValueListWire),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ReplayDoubleWire {
    Number(f64),
    Symbol(String),
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct ReplayArrayWire {
    #[serde(default)]
    values: Vec<ReplayAnyValueWire>,
}

#[derive(Deserialize)]
struct ReplayKeyValueListWire {
    #[serde(default)]
    values: Vec<ReplayKeyValueWire>,
}

struct ReplaySpanRef<'a> {
    span: &'a ReplaySpanWire,
    position: usize,
}

struct ReplayScenarioRef<'a> {
    trace_id: &'a str,
    span_id: &'a str,
    name: String,
    index: i64,
    position: usize,
}

fn decode_replay_scenarios(
    document: &ReplayOtlpDocumentWire,
    manifest: &CaptureManifestWire,
    registry: &ReplayRegistry,
) -> Result<Vec<ReplayScenario>, String> {
    if document.resource_spans.len() != 1 {
        return Err(format!(
            "reference OTLP must contain exactly one resourceSpans entry, found {}",
            document.resource_spans.len()
        ));
    }
    let resource = &document.resource_spans[0];
    let resource_attributes = replay_attribute_map(&resource.resource.attributes, "reference resource")?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.version",
        "reference resource",
        Some(CTSC_VERSION),
    )?;
    require_replay_string_attribute(&resource_attributes, "conformance.tool.name", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.tool.version", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.target.name", "reference resource", None)?;
    require_replay_string_attribute(&resource_attributes, "conformance.target.language", "reference resource", None)?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.id",
        "reference resource",
        Some(&registry.id),
    )?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.version",
        "reference resource",
        Some(&registry.version),
    )?;
    require_replay_string_attribute(
        &resource_attributes,
        "conformance.registry.digest",
        "reference resource",
        Some(&registry.digest),
    )?;

    let spans = resource
        .scope_spans
        .iter()
        .flat_map(|scope| scope.spans.iter())
        .enumerate()
        .map(|(position, span)| ReplaySpanRef { span, position })
        .collect::<Vec<_>>();
    if spans.is_empty() {
        return Err("reference OTLP contains no spans".to_string());
    }

    let mut by_id = BTreeMap::new();
    for span_ref in &spans {
        validate_replay_hex_id("trace ID", &span_ref.span.trace_id, 32)?;
        validate_replay_hex_id("span ID", &span_ref.span.span_id, 16)?;
        if by_id
            .insert((span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()), span_ref.span)
            .is_some()
        {
            return Err(format!(
                "reference OTLP contains duplicate span ID '{}' in trace '{}'",
                span_ref.span.span_id, span_ref.span.trace_id
            ));
        }
        parse_replay_time(&span_ref.span.start_time_unix_nano, &format!("span '{}'", span_ref.span.span_id))?;
    }

    let runs = spans
        .iter()
        .filter(|span_ref| span_ref.span.name == "conformance.run")
        .collect::<Vec<_>>();
    if runs.len() != 1 || !runs[0].span.parent_span_id.is_empty() {
        return Err(format!(
            "reference OTLP must contain exactly one root conformance.run span, found {}",
            runs.len()
        ));
    }

    let mut scenario_refs = Vec::new();
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name == "conformance.scenario") {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_str(), span_ref.span.parent_span_id.as_str()))
            .ok_or_else(|| format!("scenario span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name != "conformance.run" {
            return Err(format!("scenario span '{}' parent is not conformance.run", span_ref.span.span_id));
        }
        let attributes = replay_attribute_map(&span_ref.span.attributes, &format!("scenario span '{}'", span_ref.span.span_id))?;
        let name = require_replay_string_attribute(
            &attributes,
            "conformance.scenario.name",
            &format!("scenario span '{}'", span_ref.span.span_id),
            None,
        )?
        .to_string();
        let index = require_replay_integer_attribute(
            &attributes,
            "conformance.scenario.index",
            &format!("scenario span '{}'", span_ref.span.span_id),
        )?;
        if index < 0 {
            return Err(format!("scenario '{name}' has negative index {index}"));
        }
        scenario_refs.push(ReplayScenarioRef {
            trace_id: &span_ref.span.trace_id,
            span_id: &span_ref.span.span_id,
            name,
            index,
            position: span_ref.position,
        });
    }
    scenario_refs.sort_by_key(|scenario| (scenario.index, scenario.position));
    for (expected, scenario) in scenario_refs.iter().enumerate() {
        let expected = i64::try_from(expected).map_err(|_error| "reference scenario index exceeds i64".to_string())?;
        if scenario.index != expected {
            return Err(format!(
                "reference scenario indexes must be unique and contiguous from zero; expected {expected}, found {}",
                scenario.index
            ));
        }
    }
    let trace_names = scenario_refs.iter().map(|scenario| scenario.name.as_str()).collect::<Vec<_>>();
    let manifest_names = manifest.scenarios.names.iter().map(String::as_str).collect::<Vec<_>>();
    if trace_names != manifest_names {
        return Err(format!(
            "reference scenario order/names {trace_names:?} do not match capture manifest {manifest_names:?}"
        ));
    }

    let mut decoded_operations = BTreeMap::new();
    for span_ref in spans.iter().filter(|span_ref| span_ref.span.name == "conformance.operation") {
        let parent = by_id
            .get(&(span_ref.span.trace_id.as_str(), span_ref.span.parent_span_id.as_str()))
            .ok_or_else(|| format!("operation span '{}' has an unresolved parent", span_ref.span.span_id))?;
        if parent.name != "conformance.scenario" && parent.name != "conformance.operation" {
            return Err(format!(
                "operation span '{}' parent '{}' is not a scenario or operation",
                span_ref.span.span_id, parent.name
            ));
        }
        decoded_operations.insert(
            (span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()),
            decode_replay_operation(span_ref.span, registry)?,
        );
    }
    if spans.iter().any(|span_ref| span_ref.span.name == "conformance.parallel") {
        return Err("reference OTLP parallel spans are unsupported by the first replay slice".to_string());
    }

    let mut scenarios = Vec::new();
    for scenario in scenario_refs {
        let mut top_level = spans
            .iter()
            .filter(|span_ref| {
                span_ref.span.name == "conformance.operation"
                    && span_ref.span.trace_id == scenario.trace_id
                    && span_ref.span.parent_span_id == scenario.span_id
            })
            .collect::<Vec<_>>();
        top_level.sort_by_key(|span_ref| {
            (
                parse_replay_time(&span_ref.span.start_time_unix_nano, "top-level operation").unwrap_or(i64::MAX),
                span_ref.position,
            )
        });
        if top_level.is_empty() {
            return Err(format!("reference scenario '{}' contains no top-level operations", scenario.name));
        }
        let operations = top_level
            .into_iter()
            .map(|span_ref| {
                decoded_operations
                    .get(&(span_ref.span.trace_id.as_str(), span_ref.span.span_id.as_str()))
                    .cloned()
                    .ok_or_else(|| format!("operation span '{}' was not decoded", span_ref.span.span_id))
            })
            .collect::<Result<Vec<_>, String>>()?;
        scenarios.push(ReplayScenario {
            name: scenario.name,
            index: scenario.index,
            operations,
        });
    }
    Ok(scenarios)
}

fn decode_replay_operation(span: &ReplaySpanWire, registry: &ReplayRegistry) -> Result<ReplayOperation, String> {
    let location = format!("operation span '{}'", span.span_id);
    let attributes = replay_attribute_map(&span.attributes, &location)?;
    let component_id = require_replay_string_attribute(&attributes, "conformance.component.id", &location, None)?.to_string();
    let operation_name = require_replay_string_attribute(&attributes, "conformance.operation.name", &location, None)?.to_string();
    let declaration = registry
        .operations
        .iter()
        .find(|operation| operation.component_id == component_id && operation.name == operation_name)
        .ok_or_else(|| format!("reference {location} names unknown operation '{component_id}::{operation_name}'"))?;
    let input_value = attributes
        .get("conformance.operation.inputs")
        .ok_or_else(|| format!("reference {location} is missing conformance.operation.inputs"))?;
    let ReplayAnyValueWire::Kvlist(input_list) = input_value else {
        return Err(format!("reference {location} operation inputs must use kvlistValue"));
    };
    let input_values = replay_kvlist_map(input_list, &format!("{location} inputs"))?;
    if input_values.len() != declaration.inputs.len() {
        return Err(format!(
            "reference {location} input names do not match registry operation '{}::{}'",
            declaration.component_id, declaration.name
        ));
    }
    let mut inputs = Vec::new();
    for input in &declaration.inputs {
        let value = input_values.get(input.name.as_str()).ok_or_else(|| {
            format!(
                "reference {location} is missing declared input '{}' for '{}::{}'",
                input.name, declaration.component_id, declaration.name
            )
        })?;
        inputs.push(ReplayInput {
            name: input.name.clone(),
            value_type: input.value_type.clone(),
            value: decode_replay_value(value, &input.value_type, &format!("{location} input '{}'", input.name))?,
        });
    }
    for actual in input_values.keys() {
        if !declaration.inputs.iter().any(|input| input.name == *actual) {
            return Err(format!(
                "reference {location} contains undeclared input '{actual}' for '{}::{}'",
                declaration.component_id, declaration.name
            ));
        }
    }

    let mut result_count = 0_usize;
    let mut has_fault = false;
    for event in &span.events {
        match event.name.as_str() {
            "conformance.result" => {
                result_count += 1;
                let output = declaration.output.as_ref().ok_or_else(|| {
                    format!(
                        "reference {location} emits a result but registry operation '{}::{}' declares no result",
                        declaration.component_id, declaration.name
                    )
                })?;
                let event_attributes = replay_attribute_map(&event.attributes, &format!("{location} result"))?;
                let value = event_attributes
                    .get("conformance.result.value")
                    .ok_or_else(|| format!("reference {location} result is missing conformance.result.value"))?;
                let _ = decode_replay_value(value, output, &format!("{location} result"))?;
            }
            "conformance.observation" => {
                let event_attributes = replay_attribute_map(&event.attributes, &format!("{location} observation"))?;
                let name = require_replay_string_attribute(
                    &event_attributes,
                    "conformance.observation.name",
                    &format!("{location} observation"),
                    None,
                )?;
                return Err(format!(
                    "reference {location} contains unsupported observation '{name}' in the first replay slice"
                ));
            }
            "conformance.fault" => has_fault = true,
            "conformance.empty" | "conformance.error" => {
                return Err(format!("reference {location} contains unsupported terminal event '{}'", event.name));
            }
            other => return Err(format!("reference {location} contains unsupported CTSC event '{other}'")),
        }
    }
    if result_count > 1 {
        return Err(format!("reference {location} contains multiple result events"));
    }
    if declaration.output.is_some() && result_count == 0 && !has_fault {
        return Err(format!(
            "reference {location} has no result or fault for result-bearing operation '{}::{}'",
            declaration.component_id, declaration.name
        ));
    }

    Ok(ReplayOperation {
        component_id,
        operation_name,
        inputs,
        output: declaration.output.clone(),
    })
}

fn replay_attribute_map<'a>(
    attributes: &'a [ReplayKeyValueWire],
    location: &str,
) -> Result<BTreeMap<&'a str, &'a ReplayAnyValueWire>, String> {
    let mut result = BTreeMap::new();
    for attribute in attributes {
        if result.insert(attribute.key.as_str(), &attribute.value).is_some() {
            return Err(format!("{location} contains duplicate attribute '{}'", attribute.key));
        }
    }
    Ok(result)
}

fn replay_kvlist_map<'a>(list: &'a ReplayKeyValueListWire, location: &str) -> Result<BTreeMap<&'a str, &'a ReplayAnyValueWire>, String> {
    let mut result = BTreeMap::new();
    for entry in &list.values {
        if result.insert(entry.key.as_str(), &entry.value).is_some() {
            return Err(format!("{location} contains duplicate key '{}'", entry.key));
        }
    }
    Ok(result)
}

fn require_replay_string_attribute<'a>(
    attributes: &'a BTreeMap<&str, &'a ReplayAnyValueWire>,
    key: &str,
    location: &str,
    expected: Option<&str>,
) -> Result<&'a str, String> {
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing string attribute '{key}'"))?;
    let ReplayAnyValueWire::String(value) = value else {
        return Err(format!("{location} attribute '{key}' must use stringValue"));
    };
    if let Some(expected) = expected
        && value != expected
    {
        return Err(format!("{location} attribute '{key}' is '{value}', expected '{expected}'"));
    }
    Ok(value)
}

fn require_replay_integer_attribute(attributes: &BTreeMap<&str, &ReplayAnyValueWire>, key: &str, location: &str) -> Result<i64, String> {
    let value = attributes
        .get(key)
        .ok_or_else(|| format!("{location} is missing integer attribute '{key}'"))?;
    let ReplayAnyValueWire::Int(value) = value else {
        return Err(format!("{location} attribute '{key}' must use intValue"));
    };
    value
        .parse::<i64>()
        .map_err(|error| format!("{location} attribute '{key}' is not an i64 decimal integer: {error}"))
}

fn validate_replay_hex_id(label: &str, value: &str, length: usize) -> Result<(), String> {
    if value.len() != length
        || value.bytes().all(|byte| byte == b'0')
        || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "reference {label} '{value}' must be {length} lowercase hexadecimal characters and nonzero"
        ));
    }
    Ok(())
}

fn parse_replay_time(value: &str, location: &str) -> Result<i64, String> {
    let parsed = value
        .parse::<i64>()
        .map_err(|error| format!("{location} startTimeUnixNano is not an i64 decimal integer: {error}"))?;
    if parsed < 0 {
        return Err(format!("{location} startTimeUnixNano must be non-negative"));
    }
    Ok(parsed)
}

fn decode_replay_value(value: &ReplayAnyValueWire, value_type: &ReplayType, location: &str) -> Result<ReplayValue, String> {
    let ReplayType::Primitive { name } = value_type else {
        return Err(format!(
            "{location} uses unsupported structured replay type {}",
            replay_type_name(value_type)
        ));
    };
    match name.as_str() {
        "unit" => {
            let ReplayAnyValueWire::Kvlist(list) = value else {
                return Err(format!("{location} unit value must use kvlistValue"));
            };
            if !list.values.is_empty() {
                return Err(format!("{location} unit value must be an empty kvlistValue"));
            }
            Ok(ReplayValue::Unit)
        }
        "string" => match value {
            ReplayAnyValueWire::String(value) => Ok(ReplayValue::String(value.clone())),
            _ => Err(format!("{location} string value must use stringValue")),
        },
        "bool" => match value {
            ReplayAnyValueWire::Bool(value) => Ok(ReplayValue::Bool(*value)),
            _ => Err(format!("{location} bool value must use boolValue")),
        },
        "i32" => decode_replay_integer(value, location, i64::from(i32::MIN), i64::from(i32::MAX))
            .and_then(|value| i32::try_from(value).map(ReplayValue::I32).map_err(|error| error.to_string())),
        "i64" => decode_replay_integer(value, location, i64::MIN, i64::MAX).map(ReplayValue::I64),
        "u32" => decode_replay_integer(value, location, 0, i64::from(u32::MAX))
            .and_then(|value| u32::try_from(value).map(ReplayValue::U32).map_err(|error| error.to_string())),
        "u64" => {
            let ReplayAnyValueWire::String(value) = value else {
                return Err(format!("{location} u64 value must use stringValue"));
            };
            let parsed = value
                .parse::<u64>()
                .map_err(|error| format!("{location} is not a valid u64 decimal string: {error}"))?;
            if parsed.to_string() != *value {
                return Err(format!("{location} u64 value must use canonical unsigned decimal text"));
            }
            Ok(ReplayValue::U64(parsed))
        }
        "f32" => decode_replay_float(value, location, true),
        "f64" => decode_replay_float(value, location, false),
        "bytes" => Err(format!("{location} uses unsupported replay primitive 'bytes'")),
        other => Err(format!("{location} uses unknown CTSC primitive '{other}'")),
    }
}

fn decode_replay_integer(value: &ReplayAnyValueWire, location: &str, minimum: i64, maximum: i64) -> Result<i64, String> {
    let ReplayAnyValueWire::Int(value) = value else {
        return Err(format!("{location} integer value must use intValue"));
    };
    let parsed = value
        .parse::<i64>()
        .map_err(|error| format!("{location} is not a signed decimal integer: {error}"))?;
    if parsed < minimum || parsed > maximum {
        return Err(format!(
            "{location} integer value {parsed} is outside supported range {minimum}..={maximum}"
        ));
    }
    Ok(parsed)
}

fn decode_replay_float(value: &ReplayAnyValueWire, location: &str, narrow: bool) -> Result<ReplayValue, String> {
    let ReplayAnyValueWire::Double(value) = value else {
        return Err(format!("{location} floating-point value must use doubleValue"));
    };
    match value {
        ReplayDoubleWire::Number(value) if narrow => {
            let narrowed = value
                .to_string()
                .parse::<f32>()
                .map_err(|error| format!("{location} value is outside f32 range: {error}"))?;
            if f64::from(narrowed).to_bits() != value.to_bits() {
                return Err(format!("{location} value is not exactly representable as f32"));
            }
            Ok(ReplayValue::F32Bits(narrowed.to_bits()))
        }
        ReplayDoubleWire::Number(value) => Ok(ReplayValue::F64Bits(value.to_bits())),
        ReplayDoubleWire::Symbol(symbol) => {
            let bits = match (narrow, symbol.as_str()) {
                (true, "NaN") => return Ok(ReplayValue::F32Bits(f32::NAN.to_bits())),
                (true, "Infinity") => return Ok(ReplayValue::F32Bits(f32::INFINITY.to_bits())),
                (true, "-Infinity") => return Ok(ReplayValue::F32Bits(f32::NEG_INFINITY.to_bits())),
                (false, "NaN") => f64::NAN.to_bits(),
                (false, "Infinity") => f64::INFINITY.to_bits(),
                (false, "-Infinity") => f64::NEG_INFINITY.to_bits(),
                _ => return Err(format!("{location} contains invalid symbolic doubleValue '{symbol}'")),
            };
            Ok(ReplayValue::F64Bits(bits))
        }
    }
}

fn replay_type_name(value_type: &ReplayType) -> &'static str {
    match value_type {
        ReplayType::Primitive { .. } => "primitive",
        ReplayType::Named { .. } => "named",
        ReplayType::List { .. } => "list",
        ReplayType::Set { .. } => "set",
        ReplayType::Map { .. } => "map",
        ReplayType::Tuple { .. } => "tuple",
        ReplayType::Optional { .. } => "optional",
        ReplayType::Record { .. } => "record",
    }
}

fn deterministic_span_id(value: u64) -> Result<String, String> {
    if value == 0 {
        return Err("CTSC span IDs must be non-zero".to_string());
    }
    Ok(format!("{value:016x}"))
}

fn offset_time(value: i64, offset: i64) -> Result<i64, String> {
    value
        .checked_add(offset)
        .ok_or_else(|| "CTSC logical timestamp overflow".to_string())
}

fn rebase_native_operation(
    operation: &NativeOperationSpan,
    span_id: String,
    parent_span_id: String,
    time_offset: i64,
) -> Result<NativeOperationSpan, String> {
    let observations = operation
        .observations
        .iter()
        .map(|observation| {
            let mut observation = observation.clone();
            observation.time_unix_nano = offset_time(observation.time_unix_nano, time_offset)?;
            Ok(observation)
        })
        .collect::<Result<Vec<_>, String>>()?;
    let completion = operation
        .completion
        .as_ref()
        .map(|completion| match completion {
            NativeCompletion::Result {
                order,
                time_unix_nano,
                value,
            } => Ok::<NativeCompletion, String>(NativeCompletion::Result {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
                value: value.clone(),
            }),
            NativeCompletion::Fault {
                order,
                time_unix_nano,
                fault_type,
                message,
                observer,
            } => Ok::<NativeCompletion, String>(NativeCompletion::Fault {
                order: *order,
                time_unix_nano: offset_time(*time_unix_nano, time_offset)?,
                fault_type: fault_type.clone(),
                message: message.clone(),
                observer: observer.clone(),
            }),
        })
        .transpose()?;
    Ok(NativeOperationSpan {
        order: operation.order,
        span_id,
        parent_span_id,
        component_id: operation.component_id.clone(),
        operation_name: operation.operation_name.clone(),
        start_time_unix_nano: offset_time(operation.start_time_unix_nano, time_offset)?,
        end_time_unix_nano: offset_time(operation.end_time_unix_nano, time_offset)?,
        status: operation.status,
        inputs: operation.inputs.clone(),
        observations,
        completion,
    })
}

fn native_operation_span(trace_id: &str, operation: &NativeOperationSpan) -> Span {
    let mut ordered_events = operation
        .observations
        .iter()
        .map(|observation| {
            (
                observation.order,
                SpanEvent {
                    time_unix_nano: observation.time_unix_nano.to_string(),
                    name: "conformance.observation",
                    attributes: vec![
                        string_attribute("conformance.observation.name", observation.name.clone()),
                        KeyValue {
                            key: "conformance.observation.value".to_string(),
                            value: value_to_any_value(&observation.value),
                        },
                    ],
                },
            )
        })
        .collect::<Vec<_>>();
    if let Some(completion) = &operation.completion {
        ordered_events.push(match completion {
            NativeCompletion::Result {
                order,
                time_unix_nano,
                value,
            } => (
                *order,
                SpanEvent {
                    time_unix_nano: time_unix_nano.to_string(),
                    name: "conformance.result",
                    attributes: vec![KeyValue {
                        key: "conformance.result.value".to_string(),
                        value: value_to_any_value(value),
                    }],
                },
            ),
            NativeCompletion::Fault {
                order,
                time_unix_nano,
                fault_type,
                message,
                observer,
            } => (
                *order,
                SpanEvent {
                    time_unix_nano: time_unix_nano.to_string(),
                    name: "conformance.fault",
                    attributes: vec![
                        string_attribute("conformance.fault.type", fault_type.clone()),
                        string_attribute("conformance.fault.message", message.clone()),
                        string_attribute("conformance.fault.observer", observer.clone()),
                    ],
                },
            ),
        });
    }
    ordered_events.sort_by_key(|(order, _event)| *order);

    Span {
        trace_id: trace_id.to_string(),
        id: operation.span_id.clone(),
        parent_id: Some(operation.parent_span_id.clone()),
        name: "conformance.operation",
        kind: 1,
        start_time_unix_nano: operation.start_time_unix_nano.to_string(),
        end_time_unix_nano: operation.end_time_unix_nano.to_string(),
        attributes: vec![
            string_attribute("conformance.component.id", operation.component_id.clone()),
            string_attribute("conformance.operation.name", operation.operation_name.clone()),
            KeyValue {
                key: "conformance.operation.inputs".to_string(),
                value: AnyValue::Kvlist(KeyValueList {
                    values: operation
                        .inputs
                        .iter()
                        .map(|(key, value)| KeyValue {
                            key: key.clone(),
                            value: value_to_any_value(value),
                        })
                        .collect(),
                }),
            },
        ],
        events: ordered_events.into_iter().map(|(_order, event)| event).collect(),
        status: Status {
            code: status_code(operation.status),
        },
    }
}

const fn status_code(status: NativeStatus) -> i32 {
    match status {
        NativeStatus::Ok => 1,
        NativeStatus::Error => 2,
    }
}

#[spec_operation("encode_discovery_registry")]
pub fn encode_discovery_registry(
    registry_id: String,
    registry_version: String,
    component_id: String,
    discovery_json: String,
) -> CtscRegistryEncoding {
    encode_discovery_registry_result(registry_id, registry_version, component_id, &discovery_json)
        .unwrap_or_else(|reason| panic!("failed to encode discovery registry: {reason}"))
}

/// Encode one component from raw `SpecGate` discovery metadata without panicking.
///
/// # Errors
///
/// Returns an error when the discovery JSON is malformed, contains unsupported
/// types, or has no non-setup operations for `component_id`.
pub fn encode_discovery_registry_result(
    registry_id: String,
    registry_version: String,
    component_id: String,
    discovery_json: &str,
) -> Result<CtscRegistryEncoding, String> {
    let discovery: DiscoveryRegistry =
        serde_json::from_str(discovery_json).map_err(|error| format!("malformed discovery JSON: {error}"))?;
    let named_types = discovery.types.iter().map(|ty| ty.name.as_str()).collect::<BTreeSet<_>>();
    let mut operations = discovery
        .operations
        .iter()
        .filter(|operation| !operation.is_setup && operation.component == component_id)
        .map(|operation| operation.to_ctsc(&named_types))
        .collect::<Result<Vec<_>, _>>()?;

    if operations.is_empty() {
        return Err(format!("no non-setup operations found for component '{component_id}'"));
    }
    operations.sort_by(|left, right| left.name.cmp(&right.name));

    let operation_count = i32::try_from(operations.len()).map_err(|_error| "operation count exceeds i32".to_string())?;
    let document = RegistryDocument {
        format: "ctsc.registry",
        format_version: CTSC_VERSION,
        registry_id,
        version: registry_version,
        components: vec![RegistryComponent {
            id: component_id,
            operations,
            types: Vec::new(),
        }],
    };
    let registry_json = serde_json::to_string(&document).map_err(|error| format!("registry JSON serialization failed: {error}"))?;

    Ok(CtscRegistryEncoding {
        operation_count,
        type_count: 0,
        registry_json,
    })
}

#[spec_operation("encode_schema_registry")]
pub fn encode_schema_registry(registry_id: String, registry_version: String, schema_json: String) -> CtscRegistryEncoding {
    encode_schema_registry_result(registry_id, registry_version, &schema_json)
        .unwrap_or_else(|reason| panic!("failed to encode normalized schema registry: {reason}"))
}

/// Encode one normalized, setup-folded `SpecGate` schema without panicking.
///
/// # Errors
///
/// Returns an error when the schema JSON is malformed, a named type
/// declaration is unsupported, or a type reference is malformed, unsupported,
/// or names a type absent from the schema.
pub fn encode_schema_registry_result(
    registry_id: String,
    registry_version: String,
    schema_json: &str,
) -> Result<CtscRegistryEncoding, String> {
    let schema: NormalizedSchema =
        serde_json::from_str(schema_json).map_err(|error| format!("malformed normalized schema JSON: {error}"))?;
    let named_types = schema.types.iter().map(|ty| ty.name.clone()).collect::<BTreeSet<_>>();
    if named_types.len() != schema.types.len() {
        return Err("normalized schema contains duplicate type names".to_string());
    }

    let mut operations = schema
        .operations
        .iter()
        .map(|operation| operation.to_ctsc(&named_types))
        .collect::<Result<Vec<_>, _>>()?;
    operations.sort_by(|left, right| left.name.cmp(&right.name));

    let mut types = schema
        .types
        .iter()
        .map(|ty| ty.to_ctsc(&named_types))
        .collect::<Result<Vec<_>, _>>()?;
    types.sort_by(|left, right| left.name.cmp(&right.name));

    let operation_count = i32::try_from(operations.len()).map_err(|_error| "operation count exceeds i32".to_string())?;
    let type_count = i32::try_from(types.len()).map_err(|_error| "type count exceeds i32".to_string())?;
    let document = RegistryDocument {
        format: "ctsc.registry",
        format_version: CTSC_VERSION,
        registry_id,
        version: registry_version,
        components: vec![RegistryComponent {
            id: schema.component,
            operations,
            types,
        }],
    };
    let registry_json = serde_json::to_string(&document).map_err(|error| format!("registry JSON serialization failed: {error}"))?;

    Ok(CtscRegistryEncoding {
        operation_count,
        type_count,
        registry_json,
    })
}

#[derive(Deserialize)]
struct NormalizedSchema {
    component: String,
    operations: Vec<NormalizedOperation>,
    types: Vec<NormalizedType>,
}

#[derive(Deserialize)]
struct NormalizedOperation {
    name: String,
    #[serde(rename = "is_async")]
    _is_async: bool,
    inputs: Vec<NormalizedInput>,
    output: String,
}

impl NormalizedOperation {
    fn to_ctsc(&self, named_types: &BTreeSet<String>) -> Result<RegistryOperation, String> {
        let inputs = self
            .inputs
            .iter()
            .map(|input| {
                Ok(NamedValue {
                    name: input.name.clone(),
                    value_type: schema_type_ref(&input.ty, named_types)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let result = if is_schema_unit_type(&self.output) {
            None
        } else {
            Some(schema_type_ref(&self.output, named_types)?)
        };

        Ok(RegistryOperation {
            name: self.name.clone(),
            inputs,
            observations: Vec::new(),
            outcomes: RegistryOutcomes { result },
        })
    }
}

#[derive(Deserialize)]
struct NormalizedInput {
    name: String,
    ty: String,
}

#[derive(Deserialize)]
struct NormalizedType {
    name: String,
    kind: String,
    #[serde(default)]
    fields: Vec<NormalizedField>,
    #[serde(default)]
    variants: Vec<NormalizedVariant>,
}

impl NormalizedType {
    fn to_ctsc(&self, named_types: &BTreeSet<String>) -> Result<RegistryNamedType, String> {
        let shape = match self.kind.as_str() {
            "struct" => {
                if !self.variants.is_empty() {
                    return Err(format!("struct type '{}' must not declare variants", self.name));
                }
                RegistryNamedTypeShape::Record {
                    fields: normalized_fields(&self.fields, named_types)?,
                }
            }
            "enum" => {
                if !self.fields.is_empty() {
                    return Err(format!("enum type '{}' must not declare fields", self.name));
                }
                if self.variants.is_empty() {
                    return Err(format!("enum type '{}' must declare at least one variant", self.name));
                }
                RegistryNamedTypeShape::TaggedUnion {
                    variants: self
                        .variants
                        .iter()
                        .map(|variant| {
                            let payload = if variant.fields.is_empty() {
                                None
                            } else {
                                Some(RegistryTypeRef::Record {
                                    fields: normalized_fields(&variant.fields, named_types)?,
                                })
                            };
                            Ok(RegistryVariant {
                                name: variant.name.clone(),
                                payload,
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?,
                }
            }
            other => return Err(format!("unsupported normalized type kind '{other}' for '{}'", self.name)),
        };

        Ok(RegistryNamedType {
            name: self.name.clone(),
            shape,
        })
    }
}

#[derive(Deserialize)]
struct NormalizedField {
    name: String,
    ty: String,
}

#[derive(Deserialize)]
struct NormalizedVariant {
    name: String,
    #[serde(default)]
    fields: Vec<NormalizedField>,
}

fn normalized_fields(fields: &[NormalizedField], named_types: &BTreeSet<String>) -> Result<Vec<NamedValue>, String> {
    fields
        .iter()
        .map(|field| {
            Ok(NamedValue {
                name: field.name.clone(),
                value_type: schema_type_ref(&field.ty, named_types)?,
            })
        })
        .collect()
}

#[derive(Deserialize)]
struct DiscoveryRegistry {
    operations: Vec<DiscoveryOperation>,
    types: Vec<DiscoveryType>,
}

#[derive(Deserialize)]
struct DiscoveryOperation {
    name: String,
    is_setup: bool,
    #[serde(default)]
    return_type: String,
    component: String,
    params: Vec<(String, String)>,
}

impl DiscoveryOperation {
    fn to_ctsc(&self, named_types: &BTreeSet<&str>) -> Result<RegistryOperation, String> {
        let inputs = self
            .params
            .iter()
            .map(|(name, native_type)| {
                Ok(NamedValue {
                    name: name.clone(),
                    value_type: primitive_type(native_type, named_types)?,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let result = if is_unit_type(&self.return_type) {
            None
        } else {
            Some(primitive_type(&self.return_type, named_types)?)
        };

        Ok(RegistryOperation {
            name: self.name.clone(),
            inputs,
            observations: Vec::new(),
            outcomes: RegistryOutcomes { result },
        })
    }
}

#[derive(Deserialize)]
struct DiscoveryType {
    name: String,
}

fn primitive_type(native_type: &str, named_types: &BTreeSet<&str>) -> Result<RegistryTypeRef, String> {
    let normalized = normalize_native_type(native_type);
    let primitive = match normalized.as_str() {
        "()" | "unit" => "unit",
        "string" | "String" | "str" => "string",
        "bool" => "bool",
        "i32" => "i32",
        "i64" => "i64",
        "u32" => "u32",
        "u64" => "u64",
        "f32" => "f32",
        "f64" => "f64",
        "bytes" | "Vec<u8>" | "[u8]" => "bytes",
        _ if named_types.contains(normalized.as_str()) => {
            return Err(format!("named type '{normalized}' is not supported by registry encoding"));
        }
        _ => return Err(format!("unsupported type '{native_type}'")),
    };
    Ok(RegistryTypeRef::Primitive {
        name: primitive.to_string(),
    })
}

fn schema_type_ref(type_ref: &str, named_types: &BTreeSet<String>) -> Result<RegistryTypeRef, String> {
    let mut parser = TypeRefParser {
        input: type_ref,
        position: 0,
        named_types,
    };
    let parsed = parser.parse_type()?;
    parser.skip_whitespace();
    if parser.position != parser.input.len() {
        return Err(format!(
            "unexpected trailing input '{}' in type reference '{type_ref}'",
            &parser.input[parser.position..]
        ));
    }
    Ok(parsed)
}

struct TypeRefParser<'a> {
    input: &'a str,
    position: usize,
    named_types: &'a BTreeSet<String>,
}

impl TypeRefParser<'_> {
    fn parse_type(&mut self) -> Result<RegistryTypeRef, String> {
        self.skip_whitespace();
        if self.remaining().starts_with("()") {
            self.position += 2;
            return Ok(RegistryTypeRef::Primitive { name: "unit".to_string() });
        }

        let name = self.parse_identifier()?;
        self.skip_whitespace();
        if self.consume('<') {
            let arguments = self.parse_arguments()?;
            return Self::construct_generic(&name, arguments);
        }

        if is_ctsc_primitive(&name) {
            Ok(RegistryTypeRef::Primitive { name })
        } else if self.named_types.contains(&name) {
            Ok(RegistryTypeRef::Named { name })
        } else {
            Err(format!("unknown named type '{name}'"))
        }
    }

    fn parse_identifier(&mut self) -> Result<String, String> {
        self.skip_whitespace();
        let start = self.position;
        while let Some(character) = self.remaining().chars().next() {
            if character.is_alphanumeric() || matches!(character, '_' | ':' | '.') {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
        if self.position == start {
            Err(format!("expected type name at byte {}", self.position))
        } else {
            Ok(self.input[start..self.position].to_string())
        }
    }

    fn parse_arguments(&mut self) -> Result<Vec<RegistryTypeRef>, String> {
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.consume('>') {
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_type()?);
            self.skip_whitespace();
            if self.consume('>') {
                return Ok(arguments);
            }
            if self.consume(',') {
                continue;
            }
            if self.position == self.input.len() {
                return Err(format!("expected '>' at byte {}", self.position));
            }
            return Err(format!("expected ',' or '>' at byte {}", self.position));
        }
    }

    fn construct_generic(name: &str, mut arguments: Vec<RegistryTypeRef>) -> Result<RegistryTypeRef, String> {
        match name {
            "List" | "list" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::List {
                    items: Box::new(arguments.remove(0)),
                })
            }
            "Set" | "set" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::Set {
                    items: Box::new(arguments.remove(0)),
                })
            }
            "Map" | "map" => {
                expect_type_argument_count(name, &arguments, 2)?;
                let values = arguments.remove(1);
                let keys = arguments.remove(0);
                Ok(RegistryTypeRef::Map {
                    keys: Box::new(keys),
                    values: Box::new(values),
                })
            }
            "Tuple" | "tuple" => {
                if arguments.is_empty() {
                    return Err(format!("type constructor '{name}' expects at least 1 type argument"));
                }
                Ok(RegistryTypeRef::Tuple { items: arguments })
            }
            "Option" | "optional" => {
                expect_type_argument_count(name, &arguments, 1)?;
                Ok(RegistryTypeRef::Optional {
                    value: Box::new(arguments.remove(0)),
                })
            }
            other => Err(format!("unsupported type constructor '{other}'")),
        }
    }

    fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.remaining().starts_with(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.remaining().chars().next() {
            if character.is_whitespace() {
                self.position += character.len_utf8();
            } else {
                break;
            }
        }
    }

    fn remaining(&self) -> &str {
        &self.input[self.position..]
    }
}

fn expect_type_argument_count(name: &str, arguments: &[RegistryTypeRef], expected: usize) -> Result<(), String> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(format!("type constructor '{name}' expects {expected} type arguments"))
    }
}

fn is_ctsc_primitive(name: &str) -> bool {
    matches!(
        name,
        "unit" | "string" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" | "bytes"
    )
}

fn is_schema_unit_type(type_ref: &str) -> bool {
    matches!(type_ref.trim(), "" | "()" | "unit")
}

fn is_unit_type(native_type: &str) -> bool {
    let normalized = normalize_native_type(native_type);
    normalized.is_empty() || normalized == "()" || normalized == "unit"
}

fn normalize_native_type(native_type: &str) -> String {
    let mut ty = native_type.trim();
    while let Some(rest) = ty.strip_prefix('&') {
        ty = rest.trim_start();
        if let Some(lifetime_tail) = ty
            .strip_prefix('\'')
            .and_then(|rest| rest.split_once(char::is_whitespace).map(|(_, tail)| tail))
        {
            ty = lifetime_tail.trim_start();
        }
        if let Some(rest) = ty.strip_prefix("mut ") {
            ty = rest.trim_start();
        }
    }
    ty.chars().filter(|character| !character.is_whitespace()).collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryDocument {
    format: &'static str,
    format_version: &'static str,
    registry_id: String,
    version: String,
    components: Vec<RegistryComponent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryComponent {
    id: String,
    operations: Vec<RegistryOperation>,
    types: Vec<RegistryNamedType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryOperation {
    name: String,
    inputs: Vec<NamedValue>,
    observations: Vec<NamedValue>,
    outcomes: RegistryOutcomes,
}

type NamedValue = ReplayRegistryInput;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryOutcomes {
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<RegistryTypeRef>,
}

type RegistryTypeRef = ReplayType;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryVariant {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<RegistryTypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryNamedType {
    name: String,
    #[serde(flatten)]
    shape: RegistryNamedTypeShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RegistryNamedTypeShape {
    Record { fields: Vec<NamedValue> },
    TaggedUnion { variants: Vec<RegistryVariant> },
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
    name: &'static str,
    #[serde(rename = "spanId")]
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentSpanId")]
    parent_id: Option<String>,
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
    #[serde(skip_serializing_if = "Vec::is_empty")]
    values: Vec<KeyValue>,
}

#[cfg(test)]
mod tests {
    use super::*;

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

        assert_eq!(resource_spans["schemaUrl"], CTSC_SCHEMA_URL);
        assert_eq!(document["resourceSpans"][0]["scopeSpans"][0]["schemaUrl"], CTSC_SCHEMA_URL);
        assert_eq!(resource_attributes.len(), 5);
        assert_eq!(resource_attributes[0]["key"], "conformance.version");
        assert_eq!(resource_attributes[0]["value"]["stringValue"], CTSC_VERSION);
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
    fn capture_nested_rust_operations_as_native_ctsc() {
        specgate_runtime::reset();
        let result = capture_native_rust_otlp(
            "nested_double".to_string(),
            "22222222222222222222222222222222".to_string(),
            "2222222222222201".to_string(),
            "2222222222222202".to_string(),
            r#"["2222222222222203","2222222222222204"]"#.to_string(),
            "run-native-001".to_string(),
            2_000_000_000,
            100,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
            2,
        );

        assert_eq!(result.span_count, 4);
        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let resource_spans = &document["resourceSpans"][0];
        let resource_attributes = resource_spans["resource"]["attributes"].as_array().unwrap();
        let spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap();
        assert_eq!(resource_spans["schemaUrl"], CTSC_SCHEMA_URL);
        assert_eq!(resource_spans["scopeSpans"][0]["schemaUrl"], CTSC_SCHEMA_URL);
        assert_eq!(resource_attributes[0]["key"], "conformance.version");
        assert_eq!(resource_attributes[0]["value"]["stringValue"], CTSC_VERSION);
        assert_eq!(spans.len(), 4);
        assert_eq!(spans[0]["spanId"], "2222222222222201");
        assert_eq!(spans[0]["startTimeUnixNano"], "2000000000");
        assert_eq!(spans[0]["endTimeUnixNano"], "2000000900");
        assert_eq!(spans[1]["parentSpanId"], "2222222222222201");
        assert_eq!(spans[1]["endTimeUnixNano"], "2000000800");
        assert_eq!(spans[2]["parentSpanId"], "2222222222222202");
        assert_eq!(spans[2]["attributes"][0]["value"]["stringValue"], "fixture.native_capture");
        assert_eq!(spans[2]["attributes"][1]["value"]["stringValue"], "add_after_double");
        assert_eq!(
            spans[2]["attributes"][2]["value"]["kvlistValue"]["values"][0]["value"]["intValue"],
            "2"
        );
        assert_eq!(spans[2]["events"][0]["timeUnixNano"], "2000000600");
        assert_eq!(spans[2]["events"][0]["attributes"][0]["value"]["intValue"], "7");
        assert_eq!(spans[3]["parentSpanId"], "2222222222222203");
        assert_eq!(spans[3]["attributes"][1]["value"]["stringValue"], "double");
        assert_eq!(spans[3]["events"][0]["attributes"][0]["value"]["intValue"], "6");
    }

    #[test]
    fn merges_native_scenarios_into_one_deterministic_linked_run() {
        let capture = |name: &str, trace_id: &str| {
            start_native_capture(NativeCaptureConfig {
                scenario_name: name.to_string(),
                trace_id: trace_id.to_string(),
                run_span_id: "aaaaaaaaaaaaaaa1".to_string(),
                scenario_span_id: "aaaaaaaaaaaaaaa2".to_string(),
                operation_span_ids: Vec::new(),
                start_time_unix_nano: 0,
                clock_step_unix_nano: 1,
            })
            .unwrap();
            assert_eq!(double(2), 4);
            finish_native_capture().unwrap()
        };
        let captures = vec![
            capture("first", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            capture("second", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
        ];

        let first = encode_native_captures_otlp_result(
            &captures,
            "0.5.0",
            "default",
            "rust",
            "urn:ctsc:registry:fixture.native_capture",
            "0.1.0",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();
        let second = encode_native_captures_otlp_result(
            &captures,
            "0.5.0",
            "default",
            "rust",
            "urn:ctsc:registry:fixture.native_capture",
            "0.1.0",
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();

        assert_eq!(first.span_count, second.span_count);
        assert_eq!(first.otlp_json, second.otlp_json);
        assert_eq!(first.span_count, 5);
        let document: serde_json::Value = serde_json::from_str(&first.otlp_json).unwrap();
        let spans = document["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array().unwrap();
        assert_eq!(spans.iter().filter(|span| span["name"] == "conformance.run").count(), 1);
        assert_eq!(spans.iter().filter(|span| span["name"] == "conformance.scenario").count(), 2);
        assert_eq!(spans[1]["attributes"][1]["value"]["intValue"], "0");
        assert_eq!(spans[3]["attributes"][1]["value"]["intValue"], "1");
        let attributes = document["resourceSpans"][0]["resource"]["attributes"].as_array().unwrap();
        assert!(attributes.iter().any(|attribute| {
            attribute["key"] == "conformance.registry.digest"
                && attribute["value"]["stringValue"] == "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        }));
    }

    #[test]
    fn capture_present_optional_with_ctsc_wrapper() {
        specgate_runtime::reset();
        let result = capture_native_optional_otlp(
            "optional_some".to_string(),
            "44444444444444444444444444444444".to_string(),
            "4444444444444401".to_string(),
            "4444444444444402".to_string(),
            "4444444444444403".to_string(),
            "run-optional-001".to_string(),
            4_000_000_000,
            100,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
            true,
            "alice".to_string(),
        );

        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let operation = &document["resourceSpans"][0]["scopeSpans"][0]["spans"][2];
        assert_eq!(
            operation["attributes"][2]["value"]["kvlistValue"]["values"][0]["value"],
            serde_json::json!({
                "kvlistValue": {
                    "values": [{"key": "Some", "value": {"stringValue": "alice"}}]
                }
            })
        );
        assert_eq!(
            operation["events"][0]["attributes"][0]["value"],
            serde_json::json!({
                "kvlistValue": {
                    "values": [{"key": "Some", "value": {"stringValue": "alice"}}]
                }
            })
        );
    }

    #[test]
    fn capture_absent_optional_with_ctsc_wrapper() {
        specgate_runtime::reset();
        let result = capture_native_optional_otlp(
            "optional_none".to_string(),
            "55555555555555555555555555555555".to_string(),
            "5555555555555501".to_string(),
            "5555555555555502".to_string(),
            "5555555555555503".to_string(),
            "run-optional-002".to_string(),
            5_000_000_000,
            100,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
            false,
            "ignored".to_string(),
        );

        let document: serde_json::Value = serde_json::from_str(&result.otlp_json).unwrap();
        let operation = &document["resourceSpans"][0]["scopeSpans"][0]["spans"][2];
        let expected = serde_json::json!({
            "kvlistValue": {
                "values": [{"key": "None", "value": {"kvlistValue": {}}}]
            }
        });
        assert_eq!(operation["attributes"][2]["value"]["kvlistValue"]["values"][0]["value"], expected);
        assert_eq!(operation["events"][0]["attributes"][0]["value"], expected);
    }

    #[test]
    fn encode_stateless_discovery_as_registry() {
        let result = encode_test_registry(
            "fixture.stateless_add",
            r#"{"operations":[{"name":"add","is_setup":false,"is_async":false,"return_type":"i32","fills":"","component":"fixture.stateless_add","params":[["a","i32"],["b","i32"]]}],"types":[]}"#,
        );

        assert_eq!(result.operation_count, 1);
        assert_eq!(result.type_count, 0);
        assert_eq!(
            result.registry_json,
            r#"{"format":"ctsc.registry","formatVersion":"0.2.0","registryId":"urn:ctsc:registry:test:1","version":"1.0.0","components":[{"id":"fixture.stateless_add","operations":[{"name":"add","inputs":[{"name":"a","type":{"kind":"primitive","name":"i32"}},{"name":"b","type":{"kind":"primitive","name":"i32"}}],"observations":[],"outcomes":{"result":{"kind":"primitive","name":"i32"}}}],"types":[]}]}"#
        );
    }

    #[test]
    fn encode_rich_normalized_schema_as_registry() {
        let result = encode_schema_registry_result(
            "urn:ctsc:registry:fixture.rich:1".to_string(),
            "1.0.0".to_string(),
            r#"{
                "component":"fixture.rich",
                "operations":[{
                    "name":"transform",
                    "is_async":false,
                    "inputs":[
                        {"name":"person","ty":"Person"},
                        {"name":"points","ty":"List<Point>"},
                        {"name":"tags","ty":"set<string>"},
                        {"name":"scores","ty":"map<string, i64>"},
                        {"name":"pair","ty":"tuple<i32, string>"},
                        {"name":"fallback","ty":"Option<Point>"}
                    ],
                    "output":"Shape"
                }],
                "types":[
                    {"name":"Shape","kind":"enum","fields":[],"variants":[
                        {"name":"Circle","fields":[{"name":"radius","ty":"i32"}]},
                        {"name":"Rectangle","fields":[{"name":"width","ty":"i32"},{"name":"height","ty":"i32"}]},
                        {"name":"Point","fields":[]}
                    ]},
                    {"name":"Point","kind":"struct","fields":[{"name":"x","ty":"i32"},{"name":"y","ty":"i32"}],"variants":[]},
                    {"name":"Person","kind":"struct","fields":[{"name":"name","ty":"string"},{"name":"location","ty":"Point"}],"variants":[]}
                ]
            }"#,
        )
        .expect("rich normalized schema should encode");

        assert_eq!(result.operation_count, 1);
        assert_eq!(result.type_count, 3);
        assert!(!result.registry_json.contains('\n'));

        let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
        let component = &document["components"][0];
        assert_eq!(component["id"], "fixture.rich");
        assert_eq!(
            component["operations"][0]["inputs"][5]["type"],
            serde_json::json!({
                "kind": "optional",
                "value": {"kind": "named", "name": "Point"}
            })
        );
        assert_eq!(
            component["operations"][0]["inputs"][4]["type"],
            serde_json::json!({
                "kind": "tuple",
                "items": [
                    {"kind": "primitive", "name": "i32"},
                    {"kind": "primitive", "name": "string"}
                ]
            })
        );
        assert_eq!(
            component["types"][0],
            serde_json::json!({
                "name": "Person",
                "kind": "record",
                "fields": [
                    {"name": "name", "type": {"kind": "primitive", "name": "string"}},
                    {"name": "location", "type": {"kind": "named", "name": "Point"}}
                ]
            })
        );
        assert_eq!(
            component["types"][2]["variants"],
            serde_json::json!([
                {
                    "name": "Circle",
                    "payload": {
                        "kind": "record",
                        "fields": [{"name": "radius", "type": {"kind": "primitive", "name": "i32"}}]
                    }
                },
                {
                    "name": "Rectangle",
                    "payload": {
                        "kind": "record",
                        "fields": [
                            {"name": "width", "type": {"kind": "primitive", "name": "i32"}},
                            {"name": "height", "type": {"kind": "primitive", "name": "i32"}}
                        ]
                    }
                },
                {"name": "Point"}
            ])
        );
    }

    #[test]
    fn schema_registry_recurses_and_normalizes_whitespace_deterministically() {
        let compact = encode_test_schema_registry(
            r#"{"component":"selected","operations":[{"name":"nested","is_async":false,"inputs":[{"name":"value","ty":"List<Option<map<string, Set<Item>>>>"}],"output":""}],"types":[{"name":"Item","kind":"struct","fields":[],"variants":[]}]}"#,
        )
        .unwrap();
        let spaced = encode_test_schema_registry(
            r#"{"component":"selected","operations":[{"name":"nested","is_async":false,"inputs":[{"name":"value","ty":" List < optional < Map < string , set < Item > > > > "}],"output":""}],"types":[{"name":"Item","kind":"struct","fields":[],"variants":[]}]}"#,
        )
        .unwrap();

        assert_eq!(compact.registry_json, spaced.registry_json);
        let document: serde_json::Value = serde_json::from_str(&compact.registry_json).unwrap();
        assert_eq!(
            document["components"][0]["operations"][0]["inputs"][0]["type"],
            serde_json::json!({
                "kind": "list",
                "items": {
                    "kind": "optional",
                    "value": {
                        "kind": "map",
                        "keys": {"kind": "primitive", "name": "string"},
                        "values": {
                            "kind": "set",
                            "items": {"kind": "named", "name": "Item"}
                        }
                    }
                }
            })
        );
    }

    #[test]
    fn schema_registry_sorts_operations_and_types_but_preserves_declared_order() {
        let result = encode_test_schema_registry(
            r#"{
                "component":"selected",
                "operations":[
                    {"name":"zeta","is_async":false,"inputs":[{"name":"second","ty":"i64"},{"name":"first","ty":"bool"}],"output":""},
                    {"name":"alpha","is_async":false,"inputs":[],"output":"Second"}
                ],
                "types":[
                    {"name":"Second","kind":"enum","fields":[],"variants":[{"name":"B","fields":[]},{"name":"A","fields":[]}]},
                    {"name":"First","kind":"struct","fields":[{"name":"z","ty":"i32"},{"name":"a","ty":"string"}],"variants":[]}
                ]
            }"#,
        )
        .unwrap();
        let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
        let component = &document["components"][0];

        assert_eq!(component["operations"][0]["name"], "alpha");
        assert_eq!(component["operations"][1]["name"], "zeta");
        assert_eq!(component["operations"][1]["inputs"][0]["name"], "second");
        assert_eq!(component["operations"][1]["inputs"][1]["name"], "first");
        assert_eq!(component["types"][0]["name"], "First");
        assert_eq!(component["types"][0]["fields"][0]["name"], "z");
        assert_eq!(component["types"][0]["fields"][1]["name"], "a");
        assert_eq!(component["types"][1]["variants"][0]["name"], "B");
        assert_eq!(component["types"][1]["variants"][1]["name"], "A");
    }

    #[test]
    fn schema_registry_result_rejects_malformed_and_unsupported_refs_without_panicking() {
        for (ty, expected) in [
            ("List<i32", "expected '>'"),
            ("Map<string>", "expects 2 type arguments"),
            ("Result<i32, string>", "unsupported type constructor 'Result'"),
            ("Missing", "unknown named type 'Missing'"),
            ("List<i32> trailing", "unexpected trailing input"),
        ] {
            let schema = format!(
                r#"{{"component":"selected","operations":[{{"name":"bad","is_async":false,"inputs":[{{"name":"value","ty":"{ty}"}}],"output":""}}],"types":[]}}"#
            );
            let error = encode_test_schema_registry(&schema).expect_err("invalid type reference should fail");
            assert!(error.contains(expected), "expected '{expected}' in '{error}'");
        }
    }

    #[test]
    fn registry_sorts_operations_but_preserves_parameter_order_and_filters_component_setups() {
        let result = encode_test_registry(
            "selected",
            r#"{"operations":[
                {"name":"zeta","is_setup":false,"return_type":"","component":"selected","params":[["second","u64"],["first","bool"]]},
                {"name":"make_zeta","is_setup":true,"return_type":"State","component":"selected","params":[]},
                {"name":"alpha","is_setup":false,"return_type":"String","component":"selected","params":[["value","&str"]]},
                {"name":"ignored","is_setup":false,"return_type":"i32","component":"other","params":[]}
            ],"types":[]}"#,
        );
        let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
        let operations = document["components"][0]["operations"].as_array().unwrap();

        assert_eq!(operations.len(), 2);
        assert_eq!(operations[0]["name"], "alpha");
        assert_eq!(operations[1]["name"], "zeta");
        assert_eq!(operations[1]["inputs"][0]["name"], "second");
        assert_eq!(operations[1]["inputs"][1]["name"], "first");
        assert_eq!(operations[1]["outcomes"], serde_json::json!({}));
    }

    #[test]
    fn registry_maps_primitive_aliases_refs_bytes_and_unit() {
        let result = encode_test_registry(
            "selected",
            r#"{"operations":[{"name":"primitives","is_setup":false,"return_type":"()","component":"selected","params":[
                ["unit","unit"],["string","string"],["owned","String"],["slice","&'static str"],["flag","bool"],
                ["i32","i32"],["i64","i64"],["u32","u32"],["u64","u64"],["f32","f32"],["f64","f64"],
                ["bytes","Vec<u8>"],["borrowed_bytes","&[u8]"]
            ]}],"types":[]}"#,
        );
        let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
        let inputs = document["components"][0]["operations"][0]["inputs"].as_array().unwrap();
        let names = inputs
            .iter()
            .map(|input| input["type"]["name"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "unit", "string", "string", "string", "bool", "i32", "i64", "u32", "u64", "f32", "f64", "bytes", "bytes"
            ]
        );
        assert_eq!(document["components"][0]["operations"][0]["outcomes"], serde_json::json!({}));
    }

    #[test]
    fn registry_rejects_malformed_json() {
        assert_registry_error("{", "malformed discovery JSON");
    }

    #[test]
    fn registry_rejects_missing_component() {
        assert_registry_error(
            r#"{"operations":[{"name":"other","is_setup":false,"return_type":"i32","component":"other","params":[]}],"types":[]}"#,
            "no non-setup operations found for component 'selected'",
        );
    }

    #[test]
    fn registry_rejects_unsupported_type() {
        assert_registry_error(
            r#"{"operations":[{"name":"bad","is_setup":false,"return_type":"usize","component":"selected","params":[]}],"types":[]}"#,
            "unsupported type 'usize'",
        );
    }

    #[test]
    fn registry_rejects_unsupported_named_type() {
        assert_registry_error(
            r#"{"operations":[{"name":"bad","is_setup":false,"return_type":"Widget","component":"selected","params":[]}],"types":[{"name":"Widget"}]}"#,
            "named type 'Widget' is not supported",
        );
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
    fn option_trace_values_remain_some_none_kvlists() {
        let some = Value::Map(BTreeMap::from([("Some".to_string(), Value::Integer(7))]));
        let none = Value::Map(BTreeMap::from([("None".to_string(), Value::Map(BTreeMap::new()))]));

        assert_eq!(
            serde_json::to_value(value_to_any_value(&some)).unwrap(),
            serde_json::json!({
                "kvlistValue": {
                    "values": [{"key": "Some", "value": {"intValue": "7"}}]
                }
            })
        );
        assert_eq!(
            serde_json::to_value(value_to_any_value(&none)).unwrap(),
            serde_json::json!({
                "kvlistValue": {
                    "values": [{"key": "None", "value": {"kvlistValue": {}}}]
                }
            })
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

    fn encode_test_registry(component_id: &str, discovery_json: &str) -> CtscRegistryEncoding {
        encode_discovery_registry(
            "urn:ctsc:registry:test:1".to_string(),
            "1.0.0".to_string(),
            component_id.to_string(),
            discovery_json.to_string(),
        )
    }

    fn encode_test_schema_registry(schema_json: &str) -> Result<CtscRegistryEncoding, String> {
        encode_schema_registry_result("urn:ctsc:registry:test:1".to_string(), "1.0.0".to_string(), schema_json)
    }

    fn assert_registry_error(discovery_json: &str, expected: &str) {
        let panic =
            std::panic::catch_unwind(|| encode_test_registry("selected", discovery_json)).expect_err("invalid discovery should panic");
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .expect("panic should contain a string");
        assert!(message.contains(expected), "expected '{expected}' in '{message}'");
    }
}
