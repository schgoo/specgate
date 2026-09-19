use base64::Engine as _;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const CTSC_VERSION: &str = "0.2.0";
pub(crate) const CTSC_SPANS: [&str; 4] = [
    "conformance.run",
    "conformance.scenario",
    "conformance.operation",
    "conformance.parallel",
];
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

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegistryDocument {
    pub(crate) format: String,
    pub(crate) format_version: String,
    pub(crate) registry_id: String,
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) imports: Vec<RegistryImport>,
    pub(crate) components: Vec<RegistryComponent>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegistryImport {
    pub(crate) registry_id: String,
    pub(crate) version: String,
    pub(crate) digest: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub(crate) uri: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RegistryComponent {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) dependencies: Vec<ComponentRef>,
    pub(crate) operations: Vec<RegistryOperation>,
    pub(crate) types: Vec<NamedType>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ComponentRef {
    pub(crate) component_id: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub(crate) registry_id: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryOperation {
    pub(crate) name: String,
    pub(crate) inputs: Vec<NamedValue>,
    pub(crate) observations: Vec<NamedValue>,
    pub(crate) outcomes: RegistryOutcomes,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedValue {
    pub(crate) name: String,
    #[serde(rename = "type")]
    pub(crate) value_type: TypeRef,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryOutcomes {
    #[serde(default, deserialize_with = "deserialize_present")]
    pub(crate) result: Option<TypeRef>,
    #[serde(default)]
    pub(crate) empty: bool,
    #[serde(default)]
    pub(crate) errors: Vec<ErrorOutcome>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ErrorOutcome {
    pub(crate) name: String,
    #[serde(default, rename = "type", deserialize_with = "deserialize_present")]
    pub(crate) value_type: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
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
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Record { name, .. } | Self::TaggedUnion { name, .. } => name,
        }
    }

    pub(crate) fn as_type(&self) -> TypeRef {
        match self {
            Self::Record { fields, .. } => TypeRef::Record { fields: fields.clone() },
            Self::TaggedUnion { variants, .. } => TypeRef::TaggedUnion {
                variants: variants.clone(),
            },
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Variant {
    pub(crate) name: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    pub(crate) payload: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TypeRef {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(default, rename = "componentId", deserialize_with = "deserialize_present")]
        component_id: Option<String>,
        #[serde(default, rename = "registryId", deserialize_with = "deserialize_present")]
        registry_id: Option<String>,
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
pub(crate) struct RegistrySet {
    pub(crate) root_id: String,
    pub(crate) root_version: String,
    pub(crate) root_digest: String,
    pub(crate) components: BTreeMap<String, ResolvedComponent>,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedComponent {
    pub(crate) registry_id: String,
    pub(crate) component: RegistryComponent,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceDocument {
    pub(crate) spans: Vec<TraceSpan>,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceSpan {
    pub(crate) trace_id: String,
    pub(crate) span_id: String,
    pub(crate) parent_span_id: String,
    pub(crate) name: String,
    pub(crate) start_time: Option<u128>,
    pub(crate) end_time: Option<u128>,
    pub(crate) attributes: BTreeMap<String, AnyValue>,
    pub(crate) events: Vec<TraceEvent>,
    pub(crate) status_error: bool,
    pub(crate) resource_attributes: BTreeMap<String, AnyValue>,
    pub(crate) location: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TraceEvent {
    pub(crate) name: String,
    pub(crate) attributes: BTreeMap<String, AnyValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AnyValue {
    String(String),
    Bool(bool),
    Int(i64),
    Double(F64Value),
    Bytes(Vec<u8>),
    Array(Vec<AnyValue>),
    KvList(BTreeMap<String, AnyValue>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum F64Value {
    Finite(u64),
    NaN,
    Infinity,
    NegativeInfinity,
}

impl AnyValue {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"<string>\"".to_string()),
            Self::Bool(value) => value.to_string(),
            Self::Int(value) => value.to_string(),
            Self::Double(F64Value::Finite(bits)) => f64::from_bits(*bits).to_string(),
            Self::Double(F64Value::NaN) => "\"NaN\"".to_string(),
            Self::Double(F64Value::Infinity) => "\"Infinity\"".to_string(),
            Self::Double(F64Value::NegativeInfinity) => "\"-Infinity\"".to_string(),
            Self::Bytes(value) => format!("bytes:{}", base64::engine::general_purpose::STANDARD.encode(value)),
            Self::Array(values) => {
                format!("[{}]", values.iter().map(Self::display).collect::<Vec<_>>().join(","))
            }
            Self::KvList(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_else(|_| "\"<key>\"".to_string()),
                        value.display()
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    pub(crate) fn as_string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub(crate) fn as_kvlist(&self) -> Option<&BTreeMap<String, AnyValue>> {
        match self {
            Self::KvList(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CanonicalValue {
    Unit,
    String(String),
    Bool(bool),
    Integer(i128),
    Float(F64Value),
    Bytes(Vec<u8>),
    List(Vec<CanonicalValue>),
    Tuple(Vec<CanonicalValue>),
    Set(BTreeSet<CanonicalValue>),
    Map(BTreeMap<CanonicalValue, CanonicalValue>),
    Record(BTreeMap<String, CanonicalValue>),
    Variant(String, Box<CanonicalValue>),
}

impl CanonicalValue {
    pub(crate) fn display(&self) -> String {
        match self {
            Self::Unit => "{}".to_string(),
            Self::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"<string>\"".to_string()),
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
            Self::Float(F64Value::Finite(bits)) => f64::from_bits(*bits).to_string(),
            Self::Float(F64Value::NaN) => "\"NaN\"".to_string(),
            Self::Float(F64Value::Infinity) => "\"Infinity\"".to_string(),
            Self::Float(F64Value::NegativeInfinity) => "\"-Infinity\"".to_string(),
            Self::Bytes(value) => format!("bytes:{}", base64::engine::general_purpose::STANDARD.encode(value)),
            Self::List(values) | Self::Tuple(values) => {
                format!("[{}]", values.iter().map(Self::display).collect::<Vec<_>>().join(","))
            }
            Self::Set(values) => {
                format!("set[{}]", values.iter().map(Self::display).collect::<Vec<_>>().join(","))
            }
            Self::Map(values) => format!(
                "map{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{}:{}", key.display(), value.display()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Record(values) => format!(
                "{{{}}}",
                values
                    .iter()
                    .map(|(key, value)| format!("{key}:{}", value.display()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Variant(name, value) => format!("{name}({})", value.display()),
        }
    }
}
