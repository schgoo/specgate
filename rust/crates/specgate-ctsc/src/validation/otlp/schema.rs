//! Strict protobuf-shaped OTLP model and recursive validation.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TracesData {
    #[serde(default, alias = "resource_spans")]
    resource_spans: Option<Box<[ResourceSpans]>>,
}

impl TracesData {
    fn validate(&self, path: impl AsRef<str>) -> Result<(), ParseError> {
        let path = path.as_ref();
        let mut child_path = String::with_capacity(path_capacity(path, ".resourceSpans[]"));
        let mut attributes_path = String::with_capacity(path_capacity(path, ".resourceSpans[].resource.attributes"));
        let mut scope_path = String::with_capacity(path_capacity(path, ".resourceSpans[].scopeSpans[]"));
        for (index, resource) in self.resource_spans.as_deref().unwrap_or_default().iter().enumerate() {
            child_path.clear();
            write!(child_path, "{path}.resourceSpans[{index}]").expect("writing to a String cannot fail");
            resource.validate(&child_path, &mut attributes_path, &mut scope_path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceSpans {
    #[serde(default)]
    resource: Option<Resource>,
    #[serde(default, alias = "scope_spans")]
    scope_spans: Option<Box<[ScopeSpans]>>,
    #[serde(default, alias = "schema_url")]
    schema_url: Option<Box<str>>,
}

impl ResourceSpans {
    fn validate(&self, path: impl AsRef<str>, attributes_path: &mut String, child_path: &mut String) -> Result<(), ParseError> {
        let path = path.as_ref();
        if let Some(resource) = &self.resource {
            attributes_path.clear();
            write!(attributes_path, "{path}.resource.attributes").expect("writing to a String cannot fail");
            resource.validate(attributes_path)?;
        }
        for (index, scope) in self.scope_spans.as_deref().unwrap_or_default().iter().enumerate() {
            child_path.clear();
            write!(child_path, "{path}.scopeSpans[{index}]").expect("writing to a String cannot fail");
            scope.validate(&child_path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resource {
    #[serde(default)]
    attributes: Option<Box<[KeyValue]>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default, alias = "entity_refs")]
    entity_refs: Option<Box<[EntityRef]>>,
}

impl Resource {
    fn validate(&self, attributes_path: &mut String) -> Result<(), ParseError> {
        validate_attributes(self.attributes.as_deref(), attributes_path)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopeSpans {
    #[serde(default)]
    scope: Option<InstrumentationScope>,
    #[serde(default)]
    spans: Option<Box<[Span]>>,
    #[serde(default, alias = "schema_url")]
    schema_url: Option<Box<str>>,
}

impl ScopeSpans {
    fn validate(&self, path: impl AsRef<str>) -> Result<(), ParseError> {
        let path = path.as_ref();
        if let Some(scope) = &self.scope {
            let mut scope_path = String::with_capacity(path.len() + ".scope".len());
            write!(scope_path, "{path}.scope").expect("writing to a String cannot fail");
            scope.validate(&mut scope_path)?;
        }
        let mut child_path = String::with_capacity(path_capacity(path, ".spans[]"));
        let mut span_paths = SpanPaths::new(path);
        for (index, span) in self.spans.as_deref().unwrap_or_default().iter().enumerate() {
            child_path.clear();
            write!(child_path, "{path}.spans[{index}]").expect("writing to a String cannot fail");
            span.validate(&child_path, &mut span_paths)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstrumentationScope {
    #[serde(default)]
    name: Option<Box<str>>,
    #[serde(default)]
    version: Option<Box<str>>,
    #[serde(default)]
    attributes: Option<Box<[KeyValue]>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
}

impl InstrumentationScope {
    fn validate(&self, path: &mut String) -> Result<(), ParseError> {
        path.push_str(".attributes");
        validate_attributes(self.attributes.as_deref(), path)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[expect(clippy::struct_field_names, reason = "field names intentionally mirror the OTLP protobuf schema")]
struct Span {
    #[serde(default, alias = "trace_id")]
    trace_id: Option<HexBytes>,
    #[serde(default, alias = "span_id")]
    span_id: Option<HexBytes>,
    #[serde(default, alias = "trace_state")]
    trace_state: Option<Box<str>>,
    #[serde(default, alias = "parent_span_id")]
    parent_span_id: Option<HexBytes>,
    #[serde(default)]
    flags: Option<ProtoU32>,
    #[serde(default)]
    name: Option<Box<str>>,
    #[serde(default)]
    kind: Option<SpanKind>,
    #[serde(default, alias = "start_time_unix_nano")]
    start_time_unix_nano: Option<ProtoU64>,
    #[serde(default, alias = "end_time_unix_nano")]
    end_time_unix_nano: Option<ProtoU64>,
    #[serde(default)]
    attributes: Option<Box<[KeyValue]>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default)]
    events: Option<Box<[Event]>>,
    #[serde(default, alias = "dropped_events_count")]
    dropped_events_count: Option<ProtoU32>,
    #[serde(default)]
    links: Option<Box<[Link]>>,
    #[serde(default, alias = "dropped_links_count")]
    dropped_links_count: Option<ProtoU32>,
    #[serde(default)]
    status: Option<Status>,
}

struct SpanPaths {
    attributes: String,
    child: String,
}
impl SpanPaths {
    fn new(path: impl AsRef<str>) -> Self {
        let path = path.as_ref();
        Self {
            attributes: String::with_capacity(path_capacity(path, ".attributes")),
            child: String::with_capacity(path_capacity(path, ".events[].attributes")),
        }
    }
}

impl Span {
    fn validate(&self, path: impl AsRef<str>, paths: &mut SpanPaths) -> Result<(), ParseError> {
        let path = path.as_ref();
        paths.attributes.clear();
        write!(paths.attributes, "{path}.attributes").expect("writing to a String cannot fail");
        validate_attributes(self.attributes.as_deref(), &mut paths.attributes)?;
        for (index, event) in self.events.as_deref().unwrap_or_default().iter().enumerate() {
            paths.child.clear();
            write!(paths.child, "{path}.events[{index}]").expect("writing to a String cannot fail");
            paths.child.push_str(".attributes");
            validate_attributes(event.attributes.as_deref(), &mut paths.child)?;
        }
        for (index, link) in self.links.as_deref().unwrap_or_default().iter().enumerate() {
            paths.child.clear();
            write!(paths.child, "{path}.links[{index}]").expect("writing to a String cannot fail");
            paths.child.push_str(".attributes");
            validate_attributes(link.attributes.as_deref(), &mut paths.child)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Event {
    #[serde(default, alias = "time_unix_nano")]
    time_unix_nano: Option<ProtoU64>,
    #[serde(default)]
    name: Option<Box<str>>,
    #[serde(default)]
    attributes: Option<Box<[KeyValue]>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Link {
    #[serde(default, alias = "trace_id")]
    trace_id: Option<HexBytes>,
    #[serde(default, alias = "span_id")]
    span_id: Option<HexBytes>,
    #[serde(default, alias = "trace_state")]
    trace_state: Option<Box<str>>,
    #[serde(default)]
    attributes: Option<Box<[KeyValue]>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default)]
    flags: Option<ProtoU32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Status {
    #[serde(default)]
    message: Option<Box<str>>,
    #[serde(default)]
    code: Option<StatusCode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EntityRef {
    #[serde(default, alias = "schema_url")]
    schema_url: Option<Box<str>>,
    #[serde(default)]
    r#type: Option<Box<str>>,
    #[serde(default, alias = "id_keys")]
    id_keys: Option<Box<[Box<str>]>>,
    #[serde(default, alias = "description_keys")]
    description_keys: Option<Box<[Box<str>]>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyValue {
    #[serde(default)]
    key: Option<Box<str>>,
    #[serde(default)]
    value: Option<AnyValue>,
    #[serde(default, alias = "key_strindex")]
    key_strindex: Option<ProtoI32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnyValue {
    #[serde(default, alias = "string_value")]
    string_value: Option<Box<str>>,
    #[serde(default, alias = "bool_value")]
    bool_value: Option<bool>,
    #[serde(default, alias = "int_value")]
    int_value: Option<ProtoI64>,
    #[serde(default, alias = "double_value")]
    double_value: Option<ProtoF64>,
    #[serde(default, alias = "array_value")]
    array_value: Option<ArrayValue>,
    #[serde(default, alias = "kvlist_value")]
    kvlist_value: Option<KeyValueList>,
    #[serde(default, alias = "bytes_value")]
    bytes_value: Option<ProtoBytes>,
    #[serde(default, alias = "string_value_strindex")]
    string_value_strindex: Option<ProtoI32>,
}

impl AnyValue {
    fn validate(&self, path: &mut String) -> Result<(), ParseError> {
        let selected = [
            self.string_value.is_some(),
            self.bool_value.is_some(),
            self.int_value.is_some(),
            self.double_value.is_some(),
            self.array_value.is_some(),
            self.kvlist_value.is_some(),
            self.bytes_value.is_some(),
            self.string_value_strindex.is_some(),
        ]
        .into_iter()
        .filter(|selected| *selected)
        .count();
        if selected > 1 {
            return Err(ParseError::message(format!(
                "{path}: protobuf oneof AnyValue contains multiple value fields"
            )));
        }
        if let Some(array) = &self.array_value {
            let base_len = path.len();
            for (index, value) in array.values.as_deref().unwrap_or_default().iter().enumerate() {
                path.truncate(base_len);
                write!(path, ".arrayValue.values[{index}]").expect("writing to a String cannot fail");
                value.validate(path)?;
            }
            path.truncate(base_len);
        }
        if let Some(list) = &self.kvlist_value {
            let base_len = path.len();
            path.push_str(".kvlistValue.values");
            validate_attributes(list.values.as_deref(), path)?;
            path.truncate(base_len);
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArrayValue {
    #[serde(default)]
    values: Option<Box<[AnyValue]>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyValueList {
    #[serde(default)]
    values: Option<Box<[KeyValue]>>,
}

fn validate_attributes(attributes: Option<&[KeyValue]>, path: &mut String) -> Result<(), ParseError> {
    let base_len = path.len();
    for (index, attribute) in attributes.unwrap_or_default().iter().enumerate() {
        if let Some(value) = &attribute.value {
            path.truncate(base_len);
            write!(path, "[{index}].value").expect("writing to a String cannot fail");
            value.validate(path)?;
        }
    }
    path.truncate(base_len);
    Ok(())
}

#[derive(Debug)]
struct ProtoI32(i32);

impl<'de> Deserialize<'de> for ProtoI32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_integer(&value)
            .and_then(|value| i32::try_from(value).ok())
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected an int32 JSON number or numeric string"))
    }
}

#[derive(Debug)]
struct ProtoU32(u32);

impl<'de> Deserialize<'de> for ProtoU32 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_u32(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected a uint32 JSON number or numeric string"))
    }
}

#[derive(Debug)]
struct ProtoI64(i64);

impl<'de> Deserialize<'de> for ProtoI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_i64(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected an int64 JSON number or numeric string"))
    }
}

#[derive(Debug)]
struct ProtoU64(u64);

impl<'de> Deserialize<'de> for ProtoU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_u64(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected a uint64 JSON number or numeric string"))
    }
}

#[derive(Debug)]
struct ProtoF64(ParsedDouble);

impl<'de> Deserialize<'de> for ProtoF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        parse_double(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected a double JSON number or numeric/symbolic string"))
    }
}

#[derive(Debug)]
struct ProtoBytes(Box<[u8]>);

impl<'de> Deserialize<'de> for ProtoBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        decode_base64(&value)
            .map(Vec::into_boxed_slice)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected standard or URL-safe base64 with optional padding"))
    }
}

#[derive(Debug)]
struct HexBytes(Box<[u8]>);

impl<'de> Deserialize<'de> for HexBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let normalized = normalize_hex(&value).ok_or_else(|| serde::de::Error::custom("expected an even-length hexadecimal string"))?;
        let bytes: Vec<u8> = normalized
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).expect("ASCII hexadecimal");
                u8::from_str_radix(pair, 16).expect("validated hexadecimal")
            })
            .collect();
        Ok(Self(bytes.into_boxed_slice()))
    }
}

#[derive(Debug)]
struct SpanKind(i32);

impl<'de> Deserialize<'de> for SpanKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum(
            deserializer,
            [
                ("SPAN_KIND_UNSPECIFIED", 0),
                ("SPAN_KIND_INTERNAL", 1),
                ("SPAN_KIND_SERVER", 2),
                ("SPAN_KIND_CLIENT", 3),
                ("SPAN_KIND_PRODUCER", 4),
                ("SPAN_KIND_CONSUMER", 5),
            ],
        )
        .map(Self)
    }
}

#[derive(Debug)]
struct StatusCode(i32);

impl<'de> Deserialize<'de> for StatusCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserialize_enum(
            deserializer,
            [("STATUS_CODE_UNSET", 0), ("STATUS_CODE_OK", 1), ("STATUS_CODE_ERROR", 2)],
        )
        .map(Self)
    }
}

// Numeric values below are fixed by OpenTelemetry proto trace/v1/trace.proto
// (`SpanKind`) and common/v1/common.proto (`StatusCode`). Changing them breaks
// OTLP protobuf JSON wire compatibility.
fn deserialize_enum<'de, D>(deserializer: D, names: impl AsRef<[(&'static str, i32)]>) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
    let names = names.as_ref();
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(value) => names
            .iter()
            .find_map(|(name, number)| (*name == value).then_some(*number))
            .ok_or_else(|| serde::de::Error::custom(format!("unknown protobuf enum name '{value}'"))),
        Value::Number(_) => parse_integer(&value)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| serde::de::Error::custom("protobuf enum number is outside int32 range")),
        _ => Err(serde::de::Error::custom("expected a protobuf enum name or int32 number")),
    }
}

pub(super) fn validate(value: &Value) -> Result<(), ParseError> {
    let document = TracesData::deserialize(value)?;
    document.validate("$")
}
