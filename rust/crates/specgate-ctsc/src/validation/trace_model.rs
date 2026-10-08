//! Trace documents, OTLP values, and linked canonical values.

use super::{RawName, RawText};
use base64::Engine as _;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// A parsed CTSC trace containing spans after OTLP mapping and before linked validation.
#[derive(Debug, Clone)]
pub(crate) struct TraceDocument {
    /// OTLP spans normalized into CTSC trace order.
    pub(crate) spans: Vec<TraceSpan>,
}

/// A normalized CTSC span retaining ancestry, attributes, events, and status.
#[derive(Debug, Clone)]
pub(crate) struct TraceSpan {
    /// OTLP trace identity shared by related spans.
    pub(crate) trace_id: RawText,
    /// OTLP identity of this span.
    pub(crate) span_id: RawText,
    /// OTLP parent identity, empty only at the run root.
    pub(crate) parent_span_id: RawText,
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: RawName,
    /// Parsed span start timestamp when the OTLP value is valid.
    pub(crate) start_time: Option<u128>,
    /// Parsed span end timestamp when the OTLP value is valid.
    pub(crate) end_time: Option<u128>,
    /// Semantic CTSC attributes attached to this span or event.
    pub(crate) attributes: BTreeMap<String, AnyValue>,
    /// Ordered semantic events emitted by the span.
    pub(crate) events: Box<[TraceEvent]>,
    /// Parsed OTLP terminal status.
    pub(crate) status: SpanStatus,
    /// Resource-level attributes used for registry and run linkage.
    pub(crate) resource_attributes: BTreeMap<String, AnyValue>,
    /// Diagnostic path identifying the source OTLP span.
    pub(crate) location: String,
}

/// OTLP span status interpreted from symbolic or numeric JSON mapping values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanStatus {
    Unset,
    Ok,
    Error,
}

/// A normalized ordered CTSC event with its semantic attributes.
#[derive(Debug, Clone)]
pub(crate) struct TraceEvent {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: RawName,
    /// Semantic CTSC attributes attached to this span or event.
    pub(crate) attributes: BTreeMap<String, AnyValue>,
}

/// The lossless OTLP attribute algebra accepted at the trace boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AnyValue {
    /// UTF-8 text from an OTLP `stringValue`.
    String(Box<str>),
    /// Boolean data from an OTLP `boolValue`.
    Bool(bool),
    /// Signed integer data from an OTLP `intValue`.
    Int(i64),
    /// Finite or symbolic floating-point data from an OTLP `doubleValue`.
    Double(F64Value),
    /// Decoded binary data from an OTLP `bytesValue`.
    Bytes(Box<[u8]>),
    /// Ordered heterogeneous values from an OTLP `arrayValue`.
    Array(Box<[AnyValue]>),
    /// String-keyed values from an OTLP `kvlistValue`.
    KvList(BTreeMap<String, AnyValue>),
}

/// The canonical bit representation of a finite IEEE-754 binary64 value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FiniteF64(u64);

impl FiniteF64 {
    /// Preserve a finite value by its exact IEEE-754 bits; reject symbolic non-finite values.
    pub(crate) fn new(value: f64) -> Option<Self> {
        value.is_finite().then_some(Self(value.to_bits()))
    }

    /// Recover the finite floating-point value from its canonical bits.
    pub(crate) fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// A canonical finite or symbolic non-finite CTSC floating-point value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum F64Value {
    /// A finite binary64 value retained by its exact bit representation.
    Finite(FiniteF64),
    /// The CTSC symbolic `NaN` representation.
    NaN,
    /// The CTSC symbolic positive-infinity representation.
    Infinity,
    /// The CTSC symbolic negative-infinity representation.
    NegativeInfinity,
}

struct JsonWriter<'a>(&'a mut String);

impl std::io::Write for JsonWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = std::str::from_utf8(buf).map_err(std::io::Error::other)?;
        self.0.push_str(text);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn write_json(rendered: &mut String, value: impl AsRef<str>) {
    serde_json::to_writer(JsonWriter(rendered), value.as_ref()).expect("serializing a Rust string as JSON is infallible");
}

impl AnyValue {
    /// Render an OTLP value in the deterministic diagnostic notation used by validation failures.
    pub(crate) fn display(&self) -> String {
        let mut rendered = String::new();
        self.write_display(&mut rendered);
        rendered
    }

    fn write_display(&self, rendered: &mut String) {
        match self {
            Self::String(value) => write_json(rendered, value),
            Self::Bool(value) => write!(rendered, "{value}").expect("writing to a String cannot fail"),
            Self::Int(value) => write!(rendered, "{value}").expect("writing to a String cannot fail"),
            Self::Double(F64Value::Finite(bits)) => {
                write!(rendered, "{}", bits.get()).expect("writing to a String cannot fail");
            }
            Self::Double(F64Value::NaN) => rendered.push_str("\"NaN\""),
            Self::Double(F64Value::Infinity) => rendered.push_str("\"Infinity\""),
            Self::Double(F64Value::NegativeInfinity) => rendered.push_str("\"-Infinity\""),
            Self::Bytes(value) => {
                rendered.push_str("bytes:");
                base64::engine::general_purpose::STANDARD.encode_string(value, rendered);
            }
            Self::Array(values) => {
                rendered.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        rendered.push(',');
                    }
                    value.write_display(rendered);
                }
                rendered.push(']');
            }
            Self::KvList(values) => {
                rendered.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index != 0 {
                        rendered.push(',');
                    }
                    write_json(rendered, key);
                    rendered.push(':');
                    value.write_display(rendered);
                }
                rendered.push('}');
            }
        }
    }

    /// Borrow this attribute as text, returning `None` for every other OTLP kind.
    pub(crate) fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Extract this attribute as an integer, returning `None` for every other OTLP kind.
    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Borrow this attribute as a key-value list, returning `None` for every other OTLP kind.
    pub(crate) fn as_kvlist(&self) -> Option<&BTreeMap<String, AnyValue>> {
        match self {
            Self::KvList(value) => Some(value),
            _ => None,
        }
    }
}

/// An orderable linked semantic value used by deterministic comparison.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CanonicalValue {
    /// The CTSC unit value.
    Unit,
    /// UTF-8 text.
    String(Box<str>),
    /// A Boolean value.
    Bool(bool),
    /// A signed integral value normalized to the CTSC integer domain.
    Integer(i128),
    /// A finite or symbolic floating-point value.
    Float(F64Value),
    /// Opaque binary data.
    Bytes(Box<[u8]>),
    /// An ordered homogeneous sequence.
    List(Box<[CanonicalValue]>),
    /// An ordered fixed-position product.
    Tuple(Box<[CanonicalValue]>),
    /// An unordered collection of unique values.
    Set(BTreeSet<CanonicalValue>),
    /// An unordered collection of canonical key-value pairs.
    Map(BTreeMap<CanonicalValue, CanonicalValue>),
    /// A named-field product keyed by field name.
    Record(BTreeMap<String, CanonicalValue>),
    /// A named alternative and its payload.
    Variant(Box<str>, Box<CanonicalValue>),
}

impl CanonicalValue {
    /// Render a linked semantic value in stable comparison-diagnostic notation.
    pub(crate) fn display(&self) -> String {
        let mut output = String::new();
        self.write_display(&mut output);
        output
    }

    fn write_display(&self, output: &mut String) {
        match self {
            Self::Unit => output.push_str("{}"),
            Self::String(value) => write_json(output, value),
            Self::Bool(value) => write!(output, "{value}").expect("writing to a String cannot fail"),
            Self::Integer(value) => write!(output, "{value}").expect("writing to a String cannot fail"),
            Self::Float(F64Value::Finite(bits)) => write!(output, "{}", bits.get()).expect("writing to a String cannot fail"),
            Self::Float(F64Value::NaN) => output.push_str("\"NaN\""),
            Self::Float(F64Value::Infinity) => output.push_str("\"Infinity\""),
            Self::Float(F64Value::NegativeInfinity) => output.push_str("\"-Infinity\""),
            Self::Bytes(value) => {
                output.push_str("bytes:");
                base64::engine::general_purpose::STANDARD.encode_string(value, output);
            }
            Self::List(values) | Self::Tuple(values) => write_sequence(output, "[", values, ']'),
            Self::Set(values) => write_sequence(output, "set[", values, ']'),
            Self::Map(values) => {
                output.push_str("map{");
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    key.write_display(output);
                    output.push(':');
                    value.write_display(output);
                }
                output.push('}');
            }
            Self::Record(values) => {
                output.push('{');
                for (index, (key, value)) in values.iter().enumerate() {
                    if index > 0 {
                        output.push(',');
                    }
                    output.push_str(key);
                    output.push(':');
                    value.write_display(output);
                }
                output.push('}');
            }
            Self::Variant(name, value) => {
                output.push_str(name);
                output.push('(');
                value.write_display(output);
                output.push(')');
            }
        }
    }
}

fn write_sequence<'a>(output: &mut String, prefix: impl AsRef<str>, values: impl IntoIterator<Item = &'a CanonicalValue>, suffix: char) {
    output.push_str(prefix.as_ref());
    for (index, value) in values.into_iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        value.write_display(output);
    }
    output.push(suffix);
}
