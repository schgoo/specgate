//! OTLP protobuf-JSON scalar parsing and normalization.
//!
//! The helpers accept canonical protobuf JSON plus the explicitly supported
//! numeric/string forms used by CTSC traces. They reject duplicate JSON keys,
//! non-finite doubles, malformed base64, out-of-range integers, and invalid
//! hexadecimal identities before higher-level trace validation. Callers parse a
//! document with [`parse_document`], then use the typed scalar helpers while
//! preserving validation locations in the owning trace module.

#![expect(
    dead_code,
    reason = "strict protobuf-shape deserialization validates fields not otherwise inspected"
)]

use base64::Engine as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fmt::Write as _;

// An i128 has at most 39 decimal digits (including the magnitude of MIN).
const I128_MAX_DIGITS: usize = 39;

struct UniqueValue(Value);

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueVisitor).map(Self)
    }
}

struct UniqueVisitor;
impl<'de> serde::de::Visitor<'de> for UniqueVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value without duplicate object keys")
    }

    fn visit_bool<E>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::Number(v.into()))
    }
    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("JSON number must be finite"))
    }
    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Value::String(v.to_owned()))
    }
    fn visit_string<E>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::String(v))
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::SeqAccess<'de>,
    {
        let mut values = Vec::with_capacity(seq.size_hint().unwrap_or_default());
        while let Some(UniqueValue(value)) = seq.next_element()? {
            values.push(value);
        }
        values.shrink_to_fit();
        Ok(Value::Array(values))
    }
    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: serde::de::MapAccess<'de>,
    {
        let mut values = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(serde::de::Error::custom(format!("duplicate JSON key '{key}'")));
            }
            let UniqueValue(value) = map.next_value()?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

fn path_capacity(path: impl AsRef<str>, suffix: impl AsRef<str>) -> usize {
    path.as_ref().len() + suffix.as_ref().len() + usize::MAX.ilog10() as usize + 1
}

#[derive(Debug, Clone, Copy)]
/// A finite binary64 value accepted by OTLP.
pub(super) struct FiniteDouble(f64);

impl FiniteDouble {
    fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value))
    }

    /// Return the validated finite binary64 value without changing its bits.
    pub(super) fn get(self) -> f64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
/// A finite value or one of OTLP's normalized non-finite symbols.
pub(super) enum ParsedDouble {
    /// A finite JSON number or numeric string retaining exact binary64 value.
    Finite(FiniteDouble),
    /// The OTLP `NaN` symbolic string.
    NaN,
    /// The OTLP `Infinity` symbolic string.
    Infinity,
    /// The OTLP `-Infinity` symbolic string.
    NegativeInfinity,
}

/// Malformed JSON or an OTLP protobuf-JSON shape violation.
///
/// [`parse_document`] accepts one complete OTLP `TracesData` JSON object and
/// reports nested field context when protobuf aliases, oneofs, or types are invalid.
#[ohno::error]
#[display("{diagnostic}")]
pub(super) struct ParseError {
    diagnostic: String,
}

impl ParseError {
    fn message(diagnostic: impl AsRef<str>) -> Self {
        Self::new(diagnostic.as_ref().to_owned())
    }
}
impl From<serde_json::Error> for ParseError {
    fn from(source: serde_json::Error) -> Self {
        Self::caused_by("invalid OTLP JSON/protobuf mapping".to_string(), source)
    }
}

/// Parse one OTLP `TracesData` JSON document and validate its protobuf shape.
///
/// # Errors
/// Returns [`ParseError`] for malformed JSON, duplicate object keys, or values
/// that violate the OTLP protobuf JSON shape.
pub(super) fn parse_document(text: impl AsRef<str>) -> Result<Value, ParseError> {
    let text = text.as_ref();
    let value = serde_json::from_str::<UniqueValue>(text)?.0;
    schema::validate(&value)?;
    Ok(value)
}

/// Parse an OTLP integer as `i64`.
///
/// Numeric strings and integral decimal/exponent forms are accepted; fractions
/// and values outside the signed range return `None`.
pub(super) fn parse_i64(value: &Value) -> Option<i64> {
    parse_integer(value).and_then(|value| i64::try_from(value).ok())
}

/// Parse an OTLP integer as `u64`.
pub(super) fn parse_u64(value: &Value) -> Option<u64> {
    parse_integer(value).and_then(|value| u64::try_from(value).ok())
}

/// Parse an OTLP integer as `u32`.
pub(super) fn parse_u32(value: &Value) -> Option<u32> {
    parse_integer(value).and_then(|value| u32::try_from(value).ok())
}

/// Parse a finite number or normalized non-finite symbol.
pub(super) fn parse_double(value: &Value) -> Option<ParsedDouble> {
    match value {
        Value::Number(number) => number.as_f64().and_then(FiniteDouble::new).map(ParsedDouble::Finite),
        Value::String(value) => match value.as_str() {
            "NaN" => Some(ParsedDouble::NaN),
            "Infinity" => Some(ParsedDouble::Infinity),
            "-Infinity" => Some(ParsedDouble::NegativeInfinity),
            value => value.parse::<f64>().ok().and_then(FiniteDouble::new).map(ParsedDouble::Finite),
        },
        _ => None,
    }
}

/// Decode any base64 alphabet accepted by the OTLP JSON mapping.
///
/// Both standard and URL-safe alphabets are accepted with or without padding.
pub(super) fn decode_base64(value: impl AsRef<str>) -> Option<Vec<u8>> {
    // Each four base64 characters decode to at most three bytes; rounding up
    // reserves a safe output bound for padded and unpadded accepted alphabets.
    const ENCODED_QUANTUM: usize = 4;
    const DECODED_QUANTUM: usize = 3;
    let value = value.as_ref();
    let mut decoded = vec![0_u8; value.len().div_ceil(ENCODED_QUANTUM) * DECODED_QUANTUM];
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(length) = engine.decode_slice(value, &mut decoded) {
            decoded.truncate(length);
            return Some(decoded);
        }
    }
    None
}

/// Validate hexadecimal text and return its lowercase representation.
pub(super) fn normalize_hex(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref();
    (value.len().is_multiple_of(2) && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| value.to_ascii_lowercase())
}

fn parse_integer(value: &Value) -> Option<i128> {
    match value {
        Value::Number(number) => parse_decimal(number.to_string()),
        Value::String(value) => parse_decimal(value),
        _ => None,
    }
}

fn parse_decimal(value: impl AsRef<str>) -> Option<i128> {
    let value = value.as_ref();
    if let Ok(value) = value.parse::<i128>() {
        return Some(value);
    }
    let (mantissa, exponent) = match value.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (value, 0_i32),
    };
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
        if digits.len().checked_add(shift)? > I128_MAX_DIGITS {
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

mod schema;

#[cfg(test)]
mod tests {
    use super::{FiniteDouble, ParsedDouble, parse_double};
    use serde_json::json;

    #[test]
    fn finite_validity() {
        for value in [0.0, -0.0, f64::MIN, f64::MAX, f64::from_bits(1)] {
            assert_eq!(FiniteDouble::new(value).expect("finite value").get().to_bits(), value.to_bits());
        }
        for value in [f64::NAN, f64::from_bits(0x7ff8_0000_0000_0001), f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteDouble::new(value).is_none());
        }
    }

    #[test]
    fn double_symbols() {
        for input in [json!(1.25), json!("1.25")] {
            let Some(ParsedDouble::Finite(value)) = parse_double(&input) else {
                panic!("expected finite double");
            };
            assert_eq!(value.get().to_bits(), 1.25_f64.to_bits());
        }

        assert!(matches!(parse_double(&json!("NaN")), Some(ParsedDouble::NaN)));
        assert!(matches!(parse_double(&json!("Infinity")), Some(ParsedDouble::Infinity)));
        assert!(matches!(parse_double(&json!("-Infinity")), Some(ParsedDouble::NegativeInfinity)));
        assert!(parse_double(&json!("nan")).is_none());
        assert!(parse_double(&json!("inf")).is_none());
    }
    #[test]
    fn integer_bounds() {
        use super::{parse_i64, parse_u32, parse_u64};

        for (input, expected) in [
            (json!("0"), 0_i64),
            (json!("-17"), -17),
            (json!("12.00"), 12),
            (json!("12e2"), 1_200),
            (json!("1200e-2"), 12),
            (json!(i64::MAX.to_string()), i64::MAX),
            (json!(i64::MIN.to_string()), i64::MIN),
        ] {
            assert_eq!(parse_i64(&input), Some(expected), "input {input}");
        }
        for rejected in [json!("1.5"), json!("1."), json!("e2"), json!("1e-1"), json!("9223372036854775808")] {
            assert_eq!(parse_i64(&rejected), None, "input {rejected}");
        }
        assert_eq!(parse_u32(&json!(u32::MAX.to_string())), Some(u32::MAX));
        assert_eq!(parse_u32(&json!("4294967296")), None);
        assert_eq!(parse_u64(&json!(u64::MAX.to_string())), Some(u64::MAX));
        assert_eq!(parse_u64(&json!("18446744073709551616")), None);
        assert_eq!(parse_u64(&json!("-1")), None);
    }

    #[test]
    fn byte_forms() {
        use super::{decode_base64, normalize_hex};

        for encoded in ["+/8=", "+/8", "-_8=", "-_8"] {
            assert_eq!(decode_base64(encoded), Some(vec![0xfb, 0xff]), "input {encoded}");
        }
        for rejected in ["*", "a", "AA=A"] {
            assert_eq!(decode_base64(rejected), None, "input {rejected}");
        }
        assert_eq!(normalize_hex("aB00FE"), Some("ab00fe".to_string()));
        assert_eq!(normalize_hex(""), Some(String::new()));
        assert_eq!(normalize_hex("abc"), None);
        assert_eq!(normalize_hex("gg"), None);
    }

    #[test]
    fn nested_document_and_aliases() {
        let input = r#"{
            "resource_spans":[{
                "resource":{"attributes":[{"key":"service.name","value":{"stringValue":"fixture"}}]},
                "scope_spans":[{"scope":{"name":"tests","version":"1"},"spans":[{
                    "trace_id":"11111111111111111111111111111111",
                    "span_id":"1111111111111111",
                    "name":"scenario",
                    "kind":1,
                    "start_time_unix_nano":"1",
                    "end_time_unix_nano":"2",
                    "attributes":[
                        {"key":"nested","value":{"arrayValue":{"values":[{"boolValue":true}]}}},
                        {"key":"bytes","value":{"bytesValue":"+/8="}}
                    ],
                    "status":{"code":1}
                }]}]
            }]
        }"#;
        let result = super::parse_document(input);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn nested_wire_conflicts_are_rejected() {
        for input in [
            r#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"11111111111111111111111111111111","spanId":"1111111111111111","name":"x","startTimeUnixNano":"1","endTimeUnixNano":"2","attributes":[{"key":"x","value":{"stringValue":"a","intValue":"1"}}]}]}]}]}"#,
            r#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"11111111111111111111111111111111","spanId":"1111111111111111","name":"x","startTimeUnixNano":"1","endTimeUnixNano":"2","attributes":[{"key":"x","value":{"bytesValue":"*"}}]}]}]}]}"#,
        ] {
            assert!(super::parse_document(input).is_err(), "accepted malformed nested OTLP: {input}");
        }
    }

    #[test]
    fn duplicate_keys_are_rejected_at_every_depth() {
        for input in [
            r#"{"resourceSpans":[],"resourceSpans":[]}"#,
            r#"{"resourceSpans":[{"scopeSpans":[],"scopeSpans":[]}]}"#,
        ] {
            let error = super::parse_document(input).expect_err("duplicate key must fail");
            assert!(error.to_string().contains("duplicate JSON key"), "{error}");
        }
    }

    #[test]
    fn document_shape() {
        for input in [
            r#"{"unknown":true}"#,
            r#"{"resourceSpans":[{"scopeSpans":[{"spans":[{"traceId":"AA==","spanId":"AA==","name":"x","startTimeUnixNano":"1","endTimeUnixNano":"2","unknown":true}]}]}]}"#,
            r#"{"resourceSpans":[{"scopeSpans":[{"spans":"not-an-array"}]}]}"#,
        ] {
            assert!(super::parse_document(input).is_err(), "accepted malformed OTLP: {input}");
        }
    }
}
