#![allow(dead_code)]

use base64::Engine as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Copy)]
pub(super) enum ParsedDouble {
    Finite(f64),
    NaN,
    Infinity,
    NegativeInfinity,
}

pub(super) fn parse_document(text: &str) -> Result<Value, String> {
    let document = serde_json::from_str::<TracesData>(text).map_err(|error| error.to_string())?;
    document.validate("$")?;
    serde_json::from_str(text).map_err(|error| error.to_string())
}

pub(super) fn parse_i64(value: &Value) -> Option<i64> {
    parse_integer(value).and_then(|value| i64::try_from(value).ok())
}

pub(super) fn parse_u64(value: &Value) -> Option<u64> {
    parse_integer(value).and_then(|value| u64::try_from(value).ok())
}

pub(super) fn parse_u32(value: &Value) -> Option<u32> {
    parse_integer(value).and_then(|value| u32::try_from(value).ok())
}

pub(super) fn parse_double(value: &Value) -> Option<ParsedDouble> {
    match value {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()).map(ParsedDouble::Finite),
        Value::String(value) => match value.as_str() {
            "NaN" => Some(ParsedDouble::NaN),
            "Infinity" => Some(ParsedDouble::Infinity),
            "-Infinity" => Some(ParsedDouble::NegativeInfinity),
            value => value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(ParsedDouble::Finite),
        },
        _ => None,
    }
}

pub(super) fn decode_base64(value: &str) -> Option<Vec<u8>> {
    [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ]
    .into_iter()
    .find_map(|engine| engine.decode(value).ok())
}

pub(super) fn normalize_hex(value: &str) -> Option<String> {
    (value.len().is_multiple_of(2) && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| value.to_ascii_lowercase())
}

fn parse_integer(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) => parse_integer_text(&number.to_string()),
        Value::String(value) => parse_integer_text(value),
        _ => None,
    }
}

fn parse_integer_text(value: &str) -> Option<i128> {
    if let Ok(value) = value.parse::<i128>() {
        return Some(value);
    }
    let (mantissa, exponent) = value.split_once(['e', 'E']).map_or((value, 0_i32), |(mantissa, exponent)| {
        (mantissa, exponent.parse::<i32>().unwrap_or(i32::MAX))
    });
    if exponent == i32::MAX {
        return None;
    }
    let (negative, mantissa) = mantissa.strip_prefix('-').map_or((false, mantissa), |value| (true, value));
    let (whole, fraction) = mantissa.split_once('.').map_or((mantissa, ""), |parts| parts);
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || (mantissa.contains('.') && fraction.is_empty())
    {
        return None;
    }
    let mut digits = format!("{whole}{fraction}");
    let shift = exponent.checked_sub(i32::try_from(fraction.len()).ok()?)?;
    if shift >= 0 {
        let shift = usize::try_from(shift).ok()?;
        if digits.len().checked_add(shift)? > 39 {
            return None;
        }
        digits.extend(std::iter::repeat_n('0', shift));
    } else {
        let remove = usize::try_from(shift.unsigned_abs()).ok()?;
        if remove > digits.len() || !digits[digits.len() - remove..].bytes().all(|byte| byte == b'0') {
            return None;
        }
        digits.truncate(digits.len() - remove);
    }
    let digits = digits.trim_start_matches('0');
    let magnitude = if digits.is_empty() { 0 } else { digits.parse::<i128>().ok()? };
    negative.then_some(magnitude.checked_neg()?).or(Some(magnitude))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TracesData {
    #[serde(default, alias = "resource_spans")]
    resource_spans: Option<Vec<ResourceSpans>>,
}

impl TracesData {
    fn validate(&self, path: &str) -> Result<(), String> {
        for (index, resource) in self.resource_spans.as_deref().unwrap_or_default().iter().enumerate() {
            resource.validate(&format!("{path}.resourceSpans[{index}]"))?;
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
    scope_spans: Option<Vec<ScopeSpans>>,
    #[serde(default, alias = "schema_url")]
    schema_url: Option<String>,
}

impl ResourceSpans {
    fn validate(&self, path: &str) -> Result<(), String> {
        if let Some(resource) = &self.resource {
            resource.validate(&format!("{path}.resource"))?;
        }
        for (index, scope) in self.scope_spans.as_deref().unwrap_or_default().iter().enumerate() {
            scope.validate(&format!("{path}.scopeSpans[{index}]"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Resource {
    #[serde(default)]
    attributes: Option<Vec<KeyValue>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default, alias = "entity_refs")]
    entity_refs: Option<Vec<EntityRef>>,
}

impl Resource {
    fn validate(&self, path: &str) -> Result<(), String> {
        validate_attributes(self.attributes.as_deref(), &format!("{path}.attributes"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ScopeSpans {
    #[serde(default)]
    scope: Option<InstrumentationScope>,
    #[serde(default)]
    spans: Option<Vec<Span>>,
    #[serde(default, alias = "schema_url")]
    schema_url: Option<String>,
}

impl ScopeSpans {
    fn validate(&self, path: &str) -> Result<(), String> {
        if let Some(scope) = &self.scope {
            scope.validate(&format!("{path}.scope"))?;
        }
        for (index, span) in self.spans.as_deref().unwrap_or_default().iter().enumerate() {
            span.validate(&format!("{path}.spans[{index}]"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstrumentationScope {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    attributes: Option<Vec<KeyValue>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
}

impl InstrumentationScope {
    fn validate(&self, path: &str) -> Result<(), String> {
        validate_attributes(self.attributes.as_deref(), &format!("{path}.attributes"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_field_names)]
struct Span {
    #[serde(default, alias = "trace_id")]
    trace_id: Option<HexBytes>,
    #[serde(default, alias = "span_id")]
    span_id: Option<HexBytes>,
    #[serde(default, alias = "trace_state")]
    trace_state: Option<String>,
    #[serde(default, alias = "parent_span_id")]
    parent_span_id: Option<HexBytes>,
    #[serde(default)]
    flags: Option<ProtoU32>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    kind: Option<SpanKind>,
    #[serde(default, alias = "start_time_unix_nano")]
    start_time_unix_nano: Option<ProtoU64>,
    #[serde(default, alias = "end_time_unix_nano")]
    end_time_unix_nano: Option<ProtoU64>,
    #[serde(default)]
    attributes: Option<Vec<KeyValue>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default)]
    events: Option<Vec<Event>>,
    #[serde(default, alias = "dropped_events_count")]
    dropped_events_count: Option<ProtoU32>,
    #[serde(default)]
    links: Option<Vec<Link>>,
    #[serde(default, alias = "dropped_links_count")]
    dropped_links_count: Option<ProtoU32>,
    #[serde(default)]
    status: Option<Status>,
}

impl Span {
    fn validate(&self, path: &str) -> Result<(), String> {
        validate_attributes(self.attributes.as_deref(), &format!("{path}.attributes"))?;
        for (index, event) in self.events.as_deref().unwrap_or_default().iter().enumerate() {
            event.validate(&format!("{path}.events[{index}]"))?;
        }
        for (index, link) in self.links.as_deref().unwrap_or_default().iter().enumerate() {
            link.validate(&format!("{path}.links[{index}]"))?;
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
    name: Option<String>,
    #[serde(default)]
    attributes: Option<Vec<KeyValue>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
}

impl Event {
    fn validate(&self, path: &str) -> Result<(), String> {
        validate_attributes(self.attributes.as_deref(), &format!("{path}.attributes"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Link {
    #[serde(default, alias = "trace_id")]
    trace_id: Option<HexBytes>,
    #[serde(default, alias = "span_id")]
    span_id: Option<HexBytes>,
    #[serde(default, alias = "trace_state")]
    trace_state: Option<String>,
    #[serde(default)]
    attributes: Option<Vec<KeyValue>>,
    #[serde(default, alias = "dropped_attributes_count")]
    dropped_attributes_count: Option<ProtoU32>,
    #[serde(default)]
    flags: Option<ProtoU32>,
}

impl Link {
    fn validate(&self, path: &str) -> Result<(), String> {
        validate_attributes(self.attributes.as_deref(), &format!("{path}.attributes"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Status {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<StatusCode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EntityRef {
    #[serde(default, alias = "schema_url")]
    schema_url: Option<String>,
    #[serde(default)]
    r#type: Option<String>,
    #[serde(default, alias = "id_keys")]
    id_keys: Option<Vec<String>>,
    #[serde(default, alias = "description_keys")]
    description_keys: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyValue {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    value: Option<AnyValue>,
    #[serde(default, alias = "key_strindex")]
    key_strindex: Option<ProtoI32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AnyValue {
    #[serde(default, alias = "string_value")]
    string_value: Option<String>,
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
    fn validate(&self, path: &str) -> Result<(), String> {
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
            return Err(format!("{path}: protobuf oneof AnyValue contains multiple value fields"));
        }
        if let Some(array) = &self.array_value {
            for (index, value) in array.values.as_deref().unwrap_or_default().iter().enumerate() {
                value.validate(&format!("{path}.arrayValue.values[{index}]"))?;
            }
        }
        if let Some(list) = &self.kvlist_value {
            validate_attributes(list.values.as_deref(), &format!("{path}.kvlistValue.values"))?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ArrayValue {
    #[serde(default)]
    values: Option<Vec<AnyValue>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyValueList {
    #[serde(default)]
    values: Option<Vec<KeyValue>>,
}

fn validate_attributes(attributes: Option<&[KeyValue]>, path: &str) -> Result<(), String> {
    for (index, attribute) in attributes.unwrap_or_default().iter().enumerate() {
        if let Some(value) = &attribute.value {
            value.validate(&format!("{path}[{index}].value"))?;
        }
    }
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
struct ProtoBytes(Vec<u8>);

impl<'de> Deserialize<'de> for ProtoBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        decode_base64(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("expected standard or URL-safe base64 with optional padding"))
    }
}

#[derive(Debug)]
struct HexBytes(Vec<u8>);

impl<'de> Deserialize<'de> for HexBytes {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let normalized = normalize_hex(&value).ok_or_else(|| serde::de::Error::custom("expected an even-length hexadecimal string"))?;
        let bytes = normalized
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).expect("ASCII hexadecimal");
                u8::from_str_radix(pair, 16).expect("validated hexadecimal")
            })
            .collect();
        Ok(Self(bytes))
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
            &[
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
            &[("STATUS_CODE_UNSET", 0), ("STATUS_CODE_OK", 1), ("STATUS_CODE_ERROR", 2)],
        )
        .map(Self)
    }
}

fn deserialize_enum<'de, D>(deserializer: D, names: &[(&str, i32)]) -> Result<i32, D::Error>
where
    D: Deserializer<'de>,
{
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
