//! Lossless models for registry and trace documents under validation.
//!
//! [`RawText`] deliberately accepts every string because malformed document
//! values must survive parsing so later validation phases can report all
//! semantic issues with precise locations. Compact names and finite numeric
//! values are converted only after their corresponding checks succeed.

use base64::Engine as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// Unvalidated document text preserved verbatim so semantic checks can report its exact location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(transparent)]
pub(crate) struct RawText(Box<str>);
impl RawText {
    /// Borrow the original wire text without implying that semantic validation succeeded.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RawText {
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}
impl From<&str> for RawText {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl std::ops::Deref for RawText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for RawText {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl std::borrow::Borrow<str> for RawText {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RawText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl PartialEq<str> for RawText {
    fn eq(&self, other: &str) -> bool {
        self.0.as_ref() == other
    }
}
impl PartialEq<&str> for RawText {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_ref() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Unvalidated span or event name retained for unsupported-name diagnostics.
pub(crate) struct RawName(Box<str>);
impl RawName {
    /// Borrow the wire name for protocol-name validation and location-aware diagnostics.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RawName {
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}
impl From<&str> for RawName {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl std::ops::Deref for RawName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for RawName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RawName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<&RawName> for String {
    fn from(value: &RawName) -> Self {
        value.0.to_string()
    }
}
impl PartialEq<str> for RawName {
    fn eq(&self, other: &str) -> bool {
        self.0.as_ref() == other
    }
}
impl PartialEq<&str> for RawName {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_ref() == *other
    }
}
impl PartialEq<String> for RawName {
    fn eq(&self, other: &String) -> bool {
        self.0.as_ref() == other
    }
}

// These identifiers are defined by the CTSC 0.2 Registry and Trace Core
// contracts. Producers and validators must update them together.
/// Registry and trace contract version accepted by this validator.
pub(crate) const CTSC_VERSION: &str = "0.2.0";
/// Closed set of semantic CTSC span names.
pub(crate) const CTSC_SPANS: [&str; 4] = [
    "conformance.run",
    "conformance.scenario",
    "conformance.operation",
    "conformance.parallel",
];
/// Closed set of semantic CTSC event names.
pub(crate) const CTSC_EVENTS: [&str; 5] = [
    "conformance.observation",
    "conformance.result",
    "conformance.empty",
    "conformance.error",
    "conformance.fault",
];

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional registry fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A parsed CTSC registry retaining all wire fields before shape and linkage checks.
pub(crate) struct RegistryDocument {
    /// Registry document kind discriminator.
    pub(crate) format: String,
    /// CTSC registry contract version.
    pub(crate) format_version: String,
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Semantic registry version used with its identity.
    pub(crate) version: RawText,
    #[serde(default)]
    /// Imported registry identities, digests, and retrieval URIs.
    pub(crate) imports: Vec<RegistryImport>,
    /// Component definitions owned by the registry set.
    pub(crate) components: Vec<RegistryComponent>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One unresolved registry import whose identity, digest, and URI are validated later.
pub(crate) struct RegistryImport {
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Semantic registry version used with its identity.
    pub(crate) version: RawText,
    /// Expected SHA-256 digest of the imported registry bytes.
    pub(crate) digest: RawText,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Templated retrieval location for the imported registry.
    pub(crate) uri: Option<String>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional component fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One registry component retaining declaration order for deterministic-order checks.
pub(crate) struct RegistryComponent {
    /// Stable component identity within its owning registry.
    pub(crate) id: RawText,
    #[serde(default)]
    /// External component identities available to this component.
    pub(crate) dependencies: Vec<ComponentRef>,
    /// Semantic operations exposed by this component.
    pub(crate) operations: Vec<RegistryOperation>,
    /// Named semantic types exposed by this component.
    pub(crate) types: Vec<NamedType>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A local or imported component reference awaiting registry linkage.
pub(crate) struct ComponentRef {
    /// Component identity referenced across registry boundaries.
    pub(crate) component_id: RawText,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: Option<RawText>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional operation fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// An operation declaration retaining raw outcome and semantic type references.
pub(crate) struct RegistryOperation {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    /// Declared semantic operation inputs in wire order.
    pub(crate) inputs: Vec<NamedValue>,
    /// Declared observations an operation may emit.
    pub(crate) observations: Vec<NamedValue>,
    /// Declared result, empty, and error completion channels.
    pub(crate) outcomes: RegistryOutcomes,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// A named operation value paired with its unlinked semantic type.
pub(crate) struct NamedValue {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(rename = "type")]
    /// Semantic type of the named value.
    pub(crate) value_type: TypeRef,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// The result, empty-result marker, and declared errors for one operation.
pub(crate) struct RegistryOutcomes {
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Optional successful result channel type.
    pub(crate) result: Option<TypeRef>,
    #[serde(default)]
    /// Whether the operation permits explicit empty completion.
    pub(crate) empty: bool,
    #[serde(default)]
    /// Declared named error completion channels.
    pub(crate) errors: Vec<ErrorOutcome>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional error descriptions not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// A declared named error with an optional payload type.
pub(crate) struct ErrorOutcome {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(default, rename = "type", deserialize_with = "deserialize_present")]
    /// Semantic type of the named value.
    pub(crate) value_type: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[expect(dead_code, reason = "strict deserialization validates optional type descriptions and extensions")]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// A named record or enum declaration in unlinked registry form.
pub(crate) enum NamedType {
    Record {
        name: String,
        fields: Vec<NamedValue>,
        #[serde(default, deserialize_with = "deserialize_present")]
        description: Option<String>,
        #[serde(default)]
        extensions: BTreeMap<String, Value>,
    },
    TaggedUnion {
        name: String,
        variants: Vec<Variant>,
        #[serde(default, deserialize_with = "deserialize_present")]
        description: Option<String>,
        #[serde(default)]
        extensions: BTreeMap<String, Value>,
    },
}

impl NamedType {
    /// Return the declaration name used to resolve references to this named type.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Record { name, .. } | Self::TaggedUnion { name, .. } => name,
        }
    }

    /// Project the declaration into the structural type used for value validation.
    pub(crate) fn as_type(&self) -> TypeRef {
        match self {
            Self::Record { fields, .. } => TypeRef::Record { fields: fields.clone() },
            Self::TaggedUnion { variants, .. } => TypeRef::TaggedUnion {
                variants: variants.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// One enum variant represented by record fields or a tuple payload.
pub(crate) struct Variant {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Optional tuple or record payload carried by the variant.
    pub(crate) payload: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// A recursive CTSC semantic type reference before named-type linkage.
pub(crate) enum TypeRef {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(default, rename = "componentId", deserialize_with = "deserialize_present")]
        component_id: Option<RawText>,
        #[serde(default, rename = "registryId", deserialize_with = "deserialize_present")]
        registry_id: Option<RawText>,
    },
    List {
        items: Box<TypeRef>,
    },
    Set {
        items: Box<TypeRef>,
    },
    Map {
        keys: Box<TypeRef>,
        values: Box<TypeRef>,
    },
    Tuple {
        items: Vec<TypeRef>,
    },
    Record {
        fields: Vec<NamedValue>,
    },
    Optional {
        value: Box<TypeRef>,
    },
    TaggedUnion {
        variants: Vec<Variant>,
    },
}

#[derive(Debug, Clone)]
/// The root registry and imported registries indexed for exact identity resolution.
pub(crate) struct RegistrySet {
    /// Identity of the registry being validated.
    pub(crate) root_id: RawText,
    /// Version of the registry being validated.
    pub(crate) root_version: RawText,
    /// Content digest of the registry being validated.
    pub(crate) root_digest: RawText,
    /// Component definitions owned by the registry set.
    pub(crate) components: BTreeMap<RawText, ResolvedComponent>,
}

#[derive(Debug, Clone)]
/// A linked component paired with the registry that owns its named types.
pub(crate) struct ResolvedComponent {
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Linked component definition and its named-type scope.
    pub(crate) component: RegistryComponent,
}

#[derive(Debug, Clone)]
/// A parsed CTSC trace containing spans after OTLP mapping and before linked validation.
pub(crate) struct TraceDocument {
    /// OTLP spans normalized into CTSC trace order.
    pub(crate) spans: Vec<TraceSpan>,
}

#[derive(Debug, Clone)]
/// A normalized CTSC span retaining ancestry, attributes, events, and status.
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
    pub(crate) events: Vec<TraceEvent>,
    /// Whether OTLP marked this span with error status.
    pub(crate) status_error: bool,
    /// Resource-level attributes used for registry and run linkage.
    pub(crate) resource_attributes: BTreeMap<String, AnyValue>,
    /// Diagnostic path identifying the source OTLP span.
    pub(crate) location: String,
}

#[derive(Debug, Clone)]
/// A normalized ordered CTSC event with its semantic attributes.
pub(crate) struct TraceEvent {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: RawName,
    /// Semantic CTSC attributes attached to this span or event.
    pub(crate) attributes: BTreeMap<String, AnyValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
/// The lossless OTLP attribute algebra accepted at the trace boundary.
pub(crate) enum AnyValue {
    String(Box<str>),
    Bool(bool),
    Int(i64),
    Double(F64Value),
    Bytes(Box<[u8]>),
    Array(Box<[AnyValue]>),
    KvList(BTreeMap<String, AnyValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// The canonical bit representation of a finite IEEE-754 binary64 value.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// A canonical finite or symbolic non-finite CTSC floating-point value.
pub(crate) enum F64Value {
    Finite(FiniteF64),
    NaN,
    Infinity,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
/// An orderable linked semantic value used by deterministic comparison.
pub(crate) enum CanonicalValue {
    Unit,
    String(Box<str>),
    Bool(bool),
    Integer(i128),
    Float(F64Value),
    Bytes(Box<[u8]>),
    List(Box<[CanonicalValue]>),
    Tuple(Box<[CanonicalValue]>),
    Set(BTreeSet<CanonicalValue>),
    Map(BTreeMap<CanonicalValue, CanonicalValue>),
    Record(BTreeMap<String, CanonicalValue>),
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
        use std::fmt::Write as _;
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
