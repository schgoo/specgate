//! Private OTLP JSON wire DTOs used only by deterministic capture serialization.

use serde::Serialize;

// OTLP trace and span IDs are fixed-width lowercase-or-uppercase hexadecimal strings.
const TRACE_WIDTH: usize = 32;
const SPAN_WIDTH: usize = 16;
// Protobuf enum ranges defined by opentelemetry.proto.trace.v1.Span and Status.
const SPAN_KIND_MIN: i32 = 0;
const SPAN_KIND_MAX: i32 = 5;
const STATUS_CODE_MIN: i32 = 0;
const STATUS_CODE_MAX: i32 = 2;

#[ohno::error]
#[display("{diagnostic}\n{backtrace}")]
pub(super) struct WireError {
    diagnostic: &'static str,
    backtrace: std::backtrace::Backtrace,
}
impl WireError {
    fn validation(diagnostic: &'static str) -> Self {
        Self::new(diagnostic, std::backtrace::Backtrace::capture())
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct TraceId(Box<str>);
impl TraceId {
    pub(super) fn new(value: Box<str>) -> Result<Self, WireError> {
        if value.len() == TRACE_WIDTH && value.bytes().all(|byte| byte.is_ascii_hexdigit()) && value.bytes().any(|byte| byte != b'0') {
            Ok(Self(value))
        } else {
            Err(WireError::validation("OTLP trace ID must be 32 nonzero hexadecimal digits"))
        }
    }
}
impl TryFrom<Box<str>> for TraceId {
    type Error = WireError;
    fn try_from(value: Box<str>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(super) struct SpanId(Box<str>);
impl SpanId {
    pub(super) fn new(value: Box<str>) -> Result<Self, WireError> {
        if value.len() == SPAN_WIDTH && value.bytes().all(|byte| byte.is_ascii_hexdigit()) && value.bytes().any(|byte| byte != b'0') {
            Ok(Self(value))
        } else {
            Err(WireError::validation("OTLP span ID must be 16 nonzero hexadecimal digits"))
        }
    }
}
impl TryFrom<Box<str>> for SpanId {
    type Error = WireError;
    fn try_from(value: Box<str>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct UnixNanos(Box<str>);
impl UnixNanos {
    pub(super) fn new(value: Box<str>) -> Result<Self, WireError> {
        value
            .parse::<u64>()
            .map_err(|_error| WireError::validation("OTLP timestamp must be uint64 decimal nanoseconds"))?;
        Ok(Self(value))
    }
}
impl TryFrom<Box<str>> for UnixNanos {
    type Error = WireError;
    fn try_from(value: Box<str>) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct SpanKind(i32);
impl SpanKind {
    pub(super) fn new(value: i32) -> Result<Self, WireError> {
        (SPAN_KIND_MIN..=SPAN_KIND_MAX)
            .contains(&value)
            .then_some(Self(value))
            .ok_or_else(|| WireError::validation("OTLP span kind must be a defined code"))
    }
}
impl TryFrom<i32> for SpanKind {
    type Error = WireError;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct StatusCode(i32);
impl StatusCode {
    pub(super) fn new(value: i32) -> Result<Self, WireError> {
        (STATUS_CODE_MIN..=STATUS_CODE_MAX)
            .contains(&value)
            .then_some(Self(value))
            .ok_or_else(|| WireError::validation("OTLP status must be a defined code"))
    }
}
impl TryFrom<i32> for StatusCode {
    type Error = WireError;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Document {
    pub(super) resource_spans: Vec<ResourceSpans>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceSpans {
    pub(super) resource: Resource,
    pub(super) scope_spans: Vec<ScopeSpans>,
    pub(super) schema_url: Box<str>,
}

#[derive(Serialize)]
pub(super) struct Resource {
    pub(super) attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ScopeSpans {
    pub(super) scope: InstrumentationScope,
    pub(super) spans: Vec<Span>,
    pub(super) schema_url: Box<str>,
}

#[derive(Serialize)]
pub(super) struct InstrumentationScope {
    pub(super) name: &'static str,
    pub(super) version: Box<str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Span {
    pub(super) trace_id: TraceId,
    pub(super) name: &'static str,
    #[serde(rename = "spanId")]
    pub(super) id: SpanId,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "parentSpanId")]
    pub(super) parent_id: Option<SpanId>,
    pub(super) kind: SpanKind,
    #[serde(rename = "startTimeUnixNano")]
    pub(super) start_ns: UnixNanos,
    #[serde(rename = "endTimeUnixNano")]
    pub(super) end_ns: UnixNanos,
    pub(super) attributes: Vec<KeyValue>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) events: Vec<SpanEvent>,
    pub(super) status: SpanStatus,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SpanEvent {
    #[serde(skip)]
    pub(super) order: u64,
    #[serde(rename = "timeUnixNano")]
    pub(super) time_ns: UnixNanos,
    pub(super) name: &'static str,
    pub(super) attributes: Vec<KeyValue>,
}

#[derive(Serialize)]
pub(super) struct SpanStatus {
    pub(super) code: StatusCode,
}

#[derive(Serialize)]
pub(super) struct KeyValue {
    pub(super) key: Box<str>,
    pub(super) value: AnyValue,
}

#[derive(Serialize)]
pub(super) enum AnyValue {
    #[serde(rename = "stringValue")]
    String(Box<str>),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(IntValue),
    #[serde(rename = "doubleValue")]
    Double(DoubleValue),
    #[serde(rename = "arrayValue")]
    Array(ArrayValue),
    #[serde(rename = "kvlistValue")]
    Kvlist(KvList),
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct IntValue(Box<str>);
impl IntValue {
    pub(super) fn new(value: i64) -> Self {
        Self(value.to_string().into_boxed_str())
    }
}
impl From<i64> for IntValue {
    fn from(value: i64) -> Self {
        Self::new(value)
    }
}

#[derive(Serialize)]
#[serde(transparent)]
pub(super) struct DoubleValue(DoubleRepr);
impl DoubleValue {
    pub(super) fn new(value: f64) -> Self {
        // OTLP protobuf JSON requires these exact symbolic tokens for
        // non-finite doubles; changing them would break wire compatibility.
        let value = if value.is_nan() {
            DoubleRepr::Symbol("NaN")
        } else if value == f64::INFINITY {
            DoubleRepr::Symbol("Infinity")
        } else if value == f64::NEG_INFINITY {
            DoubleRepr::Symbol("-Infinity")
        } else {
            DoubleRepr::Number(value)
        };
        Self(value)
    }
}

#[derive(Serialize)]
#[serde(untagged)]
enum DoubleRepr {
    Number(f64),
    Symbol(&'static str),
}

#[derive(Serialize)]
pub(super) struct ArrayValue {
    pub(super) values: Vec<AnyValue>,
}

#[derive(Serialize)]
pub(super) struct KvList {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) values: Vec<KeyValue>,
}
