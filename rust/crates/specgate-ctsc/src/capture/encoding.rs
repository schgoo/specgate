//! Deterministic CTSC identity allocation and OTLP document encoding.

use super::otlp::{
    AnyValue, ArrayValue, Document, DoubleValue, InstrumentationScope, IntValue, KeyValue, KvList, Resource, ResourceSpans, ScopeSpans,
    Span, SpanEvent, SpanId as WireSpanId, SpanKind, SpanStatus, StatusCode, TraceId as WireTraceId, UnixNanos,
};
use super::{
    BTreeMap, Capture, Completion, EMPTY_EVENT, ERROR_EVENT, Encoding, FAULT_EVENT, ID_HEX_WIDTH, KIND_INTERNAL, Metadata, OBS_EVENT,
    OPERATION_NAME, Operation, RESULT_EVENT, RUN_NAME, SCENARIO_NAME, SCOPE_NAME, STATUS_ERROR, STATUS_OK, Status, TOOL_NAME, Value,
    attribute, candidate, ctsc, error, reference, run, time,
};

// A one-nanosecond gap makes adjacent deterministic scenario intervals unambiguously sequential.
const SCENARIO_GAP_NS: i64 = 1;
/// Hexadecimal width of an OTLP 16-byte (128-bit) trace identifier.
const TRACE_WIDTH: usize = 32;

#[derive(Clone, Copy)]
struct TraceIdentity(&'static str);
impl TraceIdentity {
    const fn const_new(value: &'static str) -> Self {
        let bytes = value.as_bytes();
        assert!(bytes.len() == TRACE_WIDTH, "CTSC trace identity must contain 32 hexadecimal digits");
        let mut index = 0;
        let mut nonzero = false;
        while index < bytes.len() {
            let byte = bytes[index];
            assert!(
                byte.is_ascii_digit() || (byte >= b'a' && byte <= b'f'),
                "CTSC trace identity must be lowercase hexadecimal"
            );
            nonzero |= byte != b'0';
            index += 1;
        }
        assert!(nonzero, "CTSC trace identity must be nonzero");
        Self(value)
    }
    fn try_new(value: &'static str) -> Result<Self, error::Error> {
        let bytes = value.as_bytes();
        if bytes.len() != TRACE_WIDTH {
            return Err(error::Error::message("trace identity has the wrong width".to_string()));
        }
        if !bytes.iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)) {
            return Err(error::Error::message("trace identity is not lowercase hexadecimal".to_string()));
        }
        if bytes.iter().all(|byte| *byte == b'0') {
            return Err(error::Error::message("trace identity is zero".to_string()));
        }
        Ok(Self(value))
    }
    const fn as_str(self) -> &'static str {
        self.0
    }
}
impl TryFrom<&'static str> for TraceIdentity {
    type Error = error::Error;
    fn try_from(value: &'static str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

#[derive(Clone, Copy)]
#[expect(clippy::struct_field_names, reason = "all values are distinct protocol identifiers")]
/// Deterministic trace, run-span, and first-child identity allocation.
///
/// Each constant selects a disjoint approved identity range. Encoding advances
/// from `first_id` with checked arithmetic, so overflow is reported as an
/// artifact error rather than wrapping.
pub(super) struct Identity {
    trace_id: TraceIdentity,
    first_id: std::num::NonZeroU64,
    run_id: &'static str,
}
impl Identity {
    /// Identity range used for reference capture artifacts.
    pub(super) const REFERENCE: Self = Self {
        trace_id: TraceIdentity::const_new(reference::TRACE_ID),
        first_id: std::num::NonZeroU64::new(reference::START_ID).expect("reference first span ID is nonzero"),
        run_id: run::CAPTURE,
    };
    /// Identity range used for replayed candidate artifacts.
    pub(super) const CANDIDATE: Self = Self {
        trace_id: TraceIdentity::const_new(candidate::TRACE_ID),
        first_id: std::num::NonZeroU64::new(candidate::START_ID).expect("candidate first span ID is nonzero"),
        run_id: run::REPLAY,
    };
}

/// Encode completed captures with the selected deterministic identity range.
///
/// Returns an error for empty capture sets, invalid intervals, or exhausted
/// identity/timestamp arithmetic. See the capture integration tests for the
/// reference and candidate byte-parity examples.
pub(super) fn encode(captures: impl AsRef<[Capture]>, metadata: &Metadata, identity: Identity) -> Result<Encoding, error::Error> {
    let captures = captures.as_ref();
    let trace_id = identity.trace_id.as_str();
    let first_id = identity.first_id;
    let run_id = identity.run_id;
    if captures.is_empty() {
        return Err("cannot encode a CTSC run without captured scenarios".to_string().into());
    }

    let trace_id = trace_id.to_string();
    let run_span_id = WireSpanId::try_from(span_id(first_id).into_boxed_str()).expect("capture values must satisfy OTLP wire constraints");
    let mut next_id = first_id
        .get()
        .checked_add(1)
        .ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
    let mut next_time = time::SCENARIO;
    let span_capacity = captures.len() + captures.iter().map(|capture| capture.operations.len()).sum::<usize>();
    let mut scenario_spans = Vec::with_capacity(span_capacity);
    let mut operation_count = 0_usize;
    let mut has_error = false;
    for (scenario_index, capture) in captures.iter().enumerate() {
        let scenario_id =
            WireSpanId::try_from(checked_id(next_id).into_boxed_str()).expect("capture values must satisfy OTLP wire constraints");
        next_id = next_id.checked_add(1).ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;

        let mut operation_ids = BTreeMap::new();
        for operation in &capture.operations {
            let span_id =
                WireSpanId::try_from(checked_id(next_id).into_boxed_str()).expect("capture values must satisfy OTLP wire constraints");
            next_id = next_id.checked_add(1).ok_or_else(|| "CTSC span ID sequence overflow".to_string())?;
            if operation_ids.insert(operation.span_id.clone(), span_id).is_some() {
                return Err(format!(
                    "native scenario '{}' contains duplicate operation span ID '{}'",
                    capture.scenario_name, operation.span_id
                )
                .into());
            }
        }

        let time_offset = next_time
            .checked_sub(capture.scenario.interval.start_ns)
            .ok_or_else(|| "CTSC scenario timestamp offset overflow".to_string())?;
        let end_time = offset_time(capture.scenario.interval.end_ns, time_offset)?;
        let scenario_index =
            i64::try_from(scenario_index).map_err(|error| error::Error::conversion("CTSC scenario index exceeds i64", error))?;
        has_error |= capture.scenario.status == Status::Error;
        scenario_spans.push(Span {
            trace_id: WireTraceId::try_from(trace_id.clone().into_boxed_str()).expect("capture values must satisfy OTLP wire constraints"),
            id: scenario_id.clone(),
            parent_id: Some(run_span_id.clone()),
            name: SCENARIO_NAME,
            kind: SpanKind::try_from(KIND_INTERNAL).expect("capture values must satisfy OTLP wire constraints"),
            start_ns: UnixNanos::try_from(next_time.to_string().into_boxed_str())
                .expect("capture values must satisfy OTLP wire constraints"),
            end_ns: UnixNanos::try_from(end_time.to_string().into_boxed_str()).expect("capture values must satisfy OTLP wire constraints"),
            attributes: vec![
                string_attribute(attribute::SCENARIO_NAME, capture.scenario_name.clone()),
                integer_attribute(attribute::SCENARIO_INDEX, scenario_index),
            ],
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::try_from(status_code(capture.scenario.status))
                    .expect("capture values must satisfy OTLP wire constraints"),
            },
        });

        for operation in &capture.operations {
            let span_id = operation_ids
                .get(&operation.span_id)
                .cloned()
                .expect("every operation received a generated span ID in the preceding loop");
            let parent_span_id = if operation.parent_id == capture.scenario.span.span_id {
                scenario_id.clone()
            } else {
                operation_ids.get(&operation.parent_id).cloned().ok_or_else(|| {
                    format!(
                        "native scenario '{}' operation '{}' has unresolved parent span '{}'",
                        capture.scenario_name, operation.operation_name, operation.parent_id
                    )
                })?
            };
            scenario_spans.push(operation_span(
                &trace_id,
                operation,
                Placement {
                    id: span_id,
                    parent_id: parent_span_id,
                    offset: time_offset,
                },
            )?);
            operation_count = operation_count
                .checked_add(1)
                .ok_or_else(|| "CTSC operation count overflow".to_string())?;
        }

        next_time = end_time
            .checked_add(SCENARIO_GAP_NS)
            .ok_or_else(|| error::Error::from("CTSC logical timestamp overflow".to_string()))?;
    }

    let run_end_time = next_time;
    let mut spans = Vec::with_capacity(scenario_spans.len() + 1);
    spans.push(Span {
        trace_id: WireTraceId::try_from(trace_id.clone().into_boxed_str()).expect("capture values must satisfy OTLP wire constraints"),
        id: run_span_id,
        parent_id: None,
        name: RUN_NAME,
        kind: SpanKind::try_from(KIND_INTERNAL).expect("capture values must satisfy OTLP wire constraints"),
        start_ns: UnixNanos::try_from(time::RUN.to_string().into_boxed_str()).expect("capture values must satisfy OTLP wire constraints"),
        end_ns: UnixNanos::try_from(run_end_time.to_string().into_boxed_str()).expect("capture values must satisfy OTLP wire constraints"),
        attributes: vec![string_attribute(attribute::RUN_ID, run_id)],
        events: Vec::new(),
        status: SpanStatus {
            code: StatusCode::try_from(status_code(if has_error { Status::Error } else { Status::Ok }))
                .expect("capture values must satisfy OTLP wire constraints"),
        },
    });
    spans.extend(scenario_spans);

    let schema_url = ctsc::SCHEMA.to_string();
    let document = Document {
        resource_spans: vec![ResourceSpans {
            resource: Resource {
                attributes: vec![
                    string_attribute(attribute::VERSION, ctsc::VERSION),
                    string_attribute(attribute::TOOL_NAME, TOOL_NAME),
                    string_attribute(attribute::TOOL_VERSION, metadata.tool_version.as_str()),
                    string_attribute(attribute::TARGET_NAME, metadata.target_name.as_str()),
                    string_attribute(attribute::TARGET_LANGUAGE, metadata.target_language.as_str()),
                    string_attribute(attribute::REGISTRY_ID, metadata.registry.id.as_str()),
                    string_attribute(attribute::REGISTRY_VERSION, metadata.registry.version.as_str()),
                    string_attribute(attribute::REGISTRY_DIGEST, metadata.registry.digest.as_str()),
                ],
            },
            scope_spans: vec![ScopeSpans {
                scope: InstrumentationScope {
                    name: SCOPE_NAME,
                    version: metadata.tool_version.clone().into_boxed_str(),
                },
                spans,
                schema_url: schema_url.clone().into_boxed_str(),
            }],
            schema_url: schema_url.into_boxed_str(),
        }],
    };
    let mut otlp_json = serde_json::to_string(&document)?;
    otlp_json.shrink_to_fit();
    let span_count = operation_count
        .checked_add(captures.len())
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| error::Error::from("native CTSC span count exceeds usize".to_string()))?;
    let span_count = i32::try_from(span_count).map_err(|error| error::Error::conversion("native CTSC span count exceeds i32", error))?;
    Ok(Encoding { span_count, otlp_json })
}

fn span_id(value: std::num::NonZeroU64) -> String {
    format!("{:0width$x}", value.get(), width = ID_HEX_WIDTH)
}
fn checked_id(value: u64) -> String {
    span_id(std::num::NonZeroU64::new(value).expect("CTSC span ID sequence must remain nonzero"))
}

fn offset_time(value: i64, offset: i64) -> Result<i64, error::Error> {
    value
        .checked_add(offset)
        .ok_or_else(|| error::Error::from("CTSC logical timestamp overflow".to_string()))
}

struct Placement {
    id: WireSpanId,
    parent_id: WireSpanId,
    offset: i64,
}

fn operation_span(trace_id: impl AsRef<str>, operation: &Operation, placement: Placement) -> Result<Span, error::Error> {
    let trace_id = trace_id.as_ref();
    let Placement { id, parent_id, offset } = placement;
    let mut events = Vec::with_capacity(operation.observations.len() + usize::from(operation.completion.is_some()));
    for observation in &operation.observations {
        events.push(SpanEvent {
            order: observation.order,
            time_ns: UnixNanos::try_from(offset_time(observation.time_ns, offset)?.to_string().into_boxed_str())
                .expect("capture values must satisfy OTLP wire constraints"),
            name: OBS_EVENT,
            attributes: vec![
                string_attribute(attribute::OBS_NAME, observation.name.clone()),
                KeyValue {
                    key: attribute::OBS_VALUE.into(),
                    value: any_value(&observation.value),
                },
            ],
        });
    }
    if let Some(completion) = &operation.completion {
        events.push(match completion {
            Completion::Result { order, time_ns, value } => SpanEvent {
                order: *order,
                time_ns: UnixNanos::try_from(offset_time(*time_ns, offset)?.to_string().into_boxed_str())
                    .expect("capture values must satisfy OTLP wire constraints"),
                name: RESULT_EVENT,
                attributes: vec![KeyValue {
                    key: attribute::RESULT_VALUE.into(),
                    value: any_value(value),
                }],
            },
            Completion::Empty { order, time_ns } => SpanEvent {
                order: *order,
                time_ns: UnixNanos::try_from(offset_time(*time_ns, offset)?.to_string().into_boxed_str())
                    .expect("capture values must satisfy OTLP wire constraints"),
                name: EMPTY_EVENT,
                attributes: Vec::new(),
            },
            Completion::Error {
                order,
                time_ns,
                name,
                value,
            } => {
                let mut attributes = Vec::with_capacity(1 + usize::from(value.is_some()));
                attributes.push(string_attribute(attribute::ERROR_NAME, name.clone()));
                if let Some(value) = value {
                    attributes.push(KeyValue {
                        key: attribute::ERROR_VALUE.into(),
                        value: any_value(value),
                    });
                }
                SpanEvent {
                    order: *order,
                    time_ns: UnixNanos::try_from(offset_time(*time_ns, offset)?.to_string().into_boxed_str())
                        .expect("capture values must satisfy OTLP wire constraints"),
                    name: ERROR_EVENT,
                    attributes,
                }
            }
            Completion::Fault {
                order,
                time_ns,
                fault_type,
                message,
                observer,
            } => SpanEvent {
                order: *order,
                time_ns: UnixNanos::try_from(offset_time(*time_ns, offset)?.to_string().into_boxed_str())
                    .expect("capture values must satisfy OTLP wire constraints"),
                name: FAULT_EVENT,
                attributes: vec![
                    string_attribute(attribute::FAULT_TYPE, fault_type.clone()),
                    string_attribute(attribute::FAULT_MESSAGE, message.clone()),
                    string_attribute(attribute::FAULT_OBSERVER, observer.clone()),
                ],
            },
        });
    }
    events.sort_by_key(|event| event.order);

    Ok(Span {
        trace_id: WireTraceId::try_from(Box::<str>::from(trace_id)).expect("capture values must satisfy OTLP wire constraints"),
        id,
        parent_id: Some(parent_id),
        name: OPERATION_NAME,
        kind: SpanKind::try_from(KIND_INTERNAL).expect("capture values must satisfy OTLP wire constraints"),
        start_ns: UnixNanos::try_from(offset_time(operation.start_ns, offset)?.to_string().into_boxed_str())
            .expect("capture values must satisfy OTLP wire constraints"),
        end_ns: UnixNanos::try_from(offset_time(operation.end_ns, offset)?.to_string().into_boxed_str())
            .expect("capture values must satisfy OTLP wire constraints"),
        attributes: vec![
            string_attribute(attribute::COMPONENT_ID, operation.component_id.clone()),
            string_attribute(attribute::OPERATION_NAME, operation.operation_name.clone()),
            KeyValue {
                key: attribute::OPERATION_INPUTS.into(),
                value: AnyValue::Kvlist(KvList {
                    values: operation
                        .inputs
                        .iter()
                        .map(|(key, value)| KeyValue {
                            key: key.clone().into_boxed_str(),
                            value: any_value(value),
                        })
                        .collect(),
                }),
            },
        ],
        events,
        status: SpanStatus {
            code: StatusCode::try_from(status_code(operation.status)).expect("capture values must satisfy OTLP wire constraints"),
        },
    })
}

const fn status_code(status: Status) -> i32 {
    match status {
        Status::Ok => STATUS_OK,
        Status::Error => STATUS_ERROR,
    }
}

fn any_value(value: &Value) -> AnyValue {
    match value {
        Value::String(value) => AnyValue::String(value.clone().into_boxed_str()),
        Value::Integer(value) => AnyValue::Int(IntValue::from(*value)),
        Value::Unsigned(value) => AnyValue::String(value.to_string().into_boxed_str()),
        Value::Float(value) if value.is_nan() => AnyValue::Double(DoubleValue::new(*value)),
        Value::Float(value) if value.is_infinite() && value.is_sign_positive() => AnyValue::Double(DoubleValue::new(*value)),
        Value::Float(value) if value.is_infinite() => AnyValue::Double(DoubleValue::new(*value)),
        Value::Float(value) => AnyValue::Double(DoubleValue::new(*value)),
        Value::Bool(value) => AnyValue::Bool(*value),
        Value::List(values) | Value::Set(values) => AnyValue::Array(ArrayValue {
            values: values.iter().map(any_value).collect(),
        }),
        Value::Map(values) => AnyValue::Kvlist(KvList {
            values: values
                .iter()
                .map(|(key, value)| KeyValue {
                    key: key.clone().into_boxed_str(),
                    value: any_value(value),
                })
                .collect(),
        }),
    }
}

fn string_attribute(key: &'static str, value: impl Into<String>) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: AnyValue::String(value.into().into_boxed_str()),
    }
}

fn integer_attribute(key: &'static str, value: i64) -> KeyValue {
    KeyValue {
        key: key.into(),
        value: AnyValue::Int(IntValue::from(value)),
    }
}
