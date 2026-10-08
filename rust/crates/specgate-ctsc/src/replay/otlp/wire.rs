//! Private serde wire DTOs for OTLP protobuf JSON.

use super::super::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OtlpDoc {
    pub(super) resource_spans: Vec<ResourceSpans>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResourceSpans {
    pub(super) resource: Resource,
    pub(super) scope_spans: Vec<ScopeSpans>,
}

#[derive(Deserialize)]
pub(super) struct Resource {
    #[serde(default)]
    pub(super) attributes: Box<[KeyValue]>,
}

#[derive(Deserialize)]
pub(super) struct ScopeSpans {
    #[serde(default)]
    pub(super) spans: Vec<Span>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Span {
    pub(super) trace_id: Box<str>,
    #[serde(rename = "spanId")]
    #[expect(clippy::struct_field_names, reason = "OTLP names this field spanId")]
    pub(super) span_id: Box<str>,
    #[serde(default, rename = "parentSpanId")]
    pub(super) parent_span_id: Box<str>,
    pub(super) name: Box<str>,
    pub(super) start_time_unix_nano: Box<str>,
    #[serde(default)]
    pub(super) attributes: Box<[KeyValue]>,
    #[serde(default)]
    pub(super) events: Box<[Event]>,
}

#[derive(Deserialize)]
pub(super) struct Event {
    pub(super) name: Box<str>,
    #[serde(default)]
    pub(super) attributes: Box<[KeyValue]>,
}

#[derive(Deserialize)]
pub(super) struct KeyValue {
    pub(super) key: Box<str>,
    pub(super) value: AnyValue,
}

#[expect(dead_code, reason = "deserialization validates fields that replay does not otherwise inspect")]
#[derive(Deserialize)]
pub(super) enum AnyValue {
    #[serde(rename = "stringValue")]
    String(String),
    #[serde(rename = "boolValue")]
    Bool(bool),
    #[serde(rename = "intValue")]
    Int(String),
    #[serde(rename = "doubleValue")]
    Double(DoubleValue),
    #[serde(rename = "bytesValue")]
    Bytes(String),
    #[serde(rename = "arrayValue")]
    Array(ArrayValue),
    #[serde(rename = "kvlistValue")]
    Kvlist(KeyValueList),
}

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum DoubleValue {
    Number(f64),
    Symbol(String),
}

#[expect(dead_code, reason = "deserialization validates array payloads before typed replay decoding")]
#[derive(Deserialize)]
pub(super) struct ArrayValue {
    #[serde(default)]
    pub(super) values: Vec<AnyValue>,
}

#[derive(Deserialize)]
pub(super) struct KeyValueList {
    #[serde(default)]
    pub(super) values: Vec<KeyValue>,
}
