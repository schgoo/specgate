//! Native CTSC semantic value algebra and primitive conversions.

use super::*;

// ---------------------------------------------------------------------------
// Value — native semantic payload.
// ---------------------------------------------------------------------------

/// Structured semantic value used by native capture and CTSC encoding.
///
/// Values preserve signedness, exact floating-point comparison semantics,
/// deterministic map/set ordering, and tagged serde representation.
///
/// # Examples
///
/// ```
/// use specgate_runtime::value::Value;
/// use std::collections::BTreeSet;
/// let list = Value::List(vec![Value::Integer(1), Value::Bool(true)]);
/// assert_eq!(list.type_name(), "list");
/// let set = Value::Set(BTreeSet::from([Value::String("stable".into())]));
/// assert_eq!(set.to_string(), "[\"stable\"]");
/// ```
#[derive(Debug, Clone)]
#[expect(clippy::exhaustive_enums, reason = "native values form a closed CTSC semantic value algebra")]
pub enum Value {
    /// UTF-8 string value.
    String(String),
    /// Signed integer value.
    Integer(i64),
    /// Unsigned integer value.
    Unsigned(u64),
    /// Floating-point value.
    Float(f64),
    /// Boolean value.
    Bool(bool),
    /// Ordered list value.
    List(Vec<Value>),
    /// String-keyed map value.
    Map(BTreeMap<String, Value>),
    /// Deterministically ordered set value.
    Set(BTreeSet<Value>),
}

pub(crate) mod wire {
    use super::Value;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Serialize)]
    #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
    enum ValueRef<'a> {
        String(&'a str),
        Integer(i64),
        Unsigned(u64),
        Float(f64),
        Bool(bool),
        List(Vec<ValueRef<'a>>),
        Map(BTreeMap<&'a str, ValueRef<'a>>),
        Set(Vec<ValueRef<'a>>),
    }

    #[derive(Deserialize)]
    #[serde(tag = "kind", content = "value", rename_all = "snake_case")]
    enum ValueOwned {
        String(String),
        Integer(i64),
        Unsigned(u64),
        Float(f64),
        Bool(bool),
        List(Vec<ValueOwned>),
        Map(BTreeMap<String, ValueOwned>),
        Set(Vec<ValueOwned>),
    }

    impl<'a> From<&'a Value> for ValueRef<'a> {
        fn from(value: &'a Value) -> Self {
            match value {
                Value::String(value) => Self::String(value),
                Value::Integer(value) => Self::Integer(*value),
                Value::Unsigned(value) => Self::Unsigned(*value),
                Value::Float(value) => Self::Float(*value),
                Value::Bool(value) => Self::Bool(*value),
                Value::List(values) => Self::List(values.iter().map(Self::from).collect()),
                Value::Map(values) => Self::Map(values.iter().map(|(key, value)| (key.as_str(), Self::from(value))).collect()),
                Value::Set(values) => Self::Set(values.iter().map(Self::from).collect()),
            }
        }
    }

    impl From<ValueOwned> for Value {
        fn from(value: ValueOwned) -> Self {
            match value {
                ValueOwned::String(value) => Self::String(value),
                ValueOwned::Integer(value) => Self::Integer(value),
                ValueOwned::Unsigned(value) => Self::Unsigned(value),
                ValueOwned::Float(value) => Self::Float(value),
                ValueOwned::Bool(value) => Self::Bool(value),
                ValueOwned::List(values) => Self::List(values.into_iter().map(Self::from).collect()),
                ValueOwned::Map(values) => Self::Map(values.into_iter().map(|(key, value)| (key, Self::from(value))).collect()),
                ValueOwned::Set(values) => Self::Set(values.into_iter().map(Self::from).collect()),
            }
        }
    }

    pub(crate) fn serialize<S: serde::Serializer>(value: &Value, serializer: S) -> Result<S::Ok, S::Error> {
        ValueRef::from(value).serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
        ValueOwned::deserialize(deserializer).map(Value::from)
    }

    pub(crate) mod optional {
        use super::{Deserialize, Serialize, Value, ValueOwned, ValueRef};
        #[expect(
            clippy::ref_option,
            reason = "serde serialize_with requires a reference to the annotated Option field"
        )]
        pub(crate) fn serialize<S: serde::Serializer>(value: &Option<Value>, serializer: S) -> Result<S::Ok, S::Error> {
            value.as_ref().map(ValueRef::from).serialize(serializer)
        }
        pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
            Option::<ValueOwned>::deserialize(deserializer).map(|value| value.map(Value::from))
        }
    }

    pub(crate) mod map {
        use super::{BTreeMap, Deserialize, Serialize, Value, ValueOwned, ValueRef};
        pub(crate) fn serialize<S: serde::Serializer>(value: &BTreeMap<String, Value>, serializer: S) -> Result<S::Ok, S::Error> {
            value
                .iter()
                .map(|(key, value)| (key.as_str(), ValueRef::from(value)))
                .collect::<BTreeMap<_, _>>()
                .serialize(serializer)
        }
        #[expect(
            clippy::trivially_copy_pass_by_ref,
            reason = "serde serialize_with passes a reference to the annotated borrowed map field"
        )]
        pub(crate) fn serialize_ref<S: serde::Serializer>(value: &&BTreeMap<String, Value>, serializer: S) -> Result<S::Ok, S::Error> {
            serialize(value, serializer)
        }
        pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, Value>, D::Error> {
            BTreeMap::<String, ValueOwned>::deserialize(deserializer)
                .map(|values| values.into_iter().map(|(key, value)| (key, Value::from(value))).collect())
        }
    }
}

impl Value {
    /// Return the stable native semantic kind name.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "string",
            Value::Integer(_) => "int",
            Value::Unsigned(_) => "uint",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
        }
    }
}

fn variant_rank(v: &Value) -> u8 {
    // These ranks define the stable cross-variant order used by semantic sets;
    // changing them changes deterministic capture and comparison ordering.
    const BOOL_RANK: u8 = 0;
    const INTEGER_RANK: u8 = 1;
    const UNSIGNED_RANK: u8 = 2;
    const FLOAT_RANK: u8 = 3;
    const STRING_RANK: u8 = 4;
    const LIST_RANK: u8 = 5;
    const SET_RANK: u8 = 6;
    const MAP_RANK: u8 = 7;
    match v {
        Value::Bool(_) => BOOL_RANK,
        Value::Integer(_) => INTEGER_RANK,
        Value::Unsigned(_) => UNSIGNED_RANK,
        Value::Float(_) => FLOAT_RANK,
        Value::String(_) => STRING_RANK,
        Value::List(_) => LIST_RANK,
        Value::Set(_) => SET_RANK,
        Value::Map(_) => MAP_RANK,
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Unsigned(a), Value::Unsigned(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            _ => false,
        }
    }
}
impl Eq for Value {}

impl PartialOrd for Value {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Value {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::Integer(a), Value::Integer(b)) => a.cmp(b),
            (Value::Unsigned(a), Value::Unsigned(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
            (Value::List(a), Value::List(b)) => a.cmp(b),
            (Value::Map(a), Value::Map(b)) => a.cmp(b),
            (Value::Set(a), Value::Set(b)) => a.cmp(b),
            (a, b) => variant_rank(a).cmp(&variant_rank(b)),
        }
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::String(s) => write!(f, "{s}"),
            Value::Integer(i) => write!(f, "{i}"),
            Value::Unsigned(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_atom(f, v)?;
                }
                write!(f, "]")
            }
            Value::Set(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_atom(f, v)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "\"{k}\":")?;
                    write_atom(f, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_atom(f: &mut std::fmt::Formatter<'_>, v: &Value) -> std::fmt::Result {
    match v {
        Value::String(s) => write!(f, "\"{s}\""),
        other => write!(f, "{other}"),
    }
}

// --- conversions used by tests and macro-generated code -------------------

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}
impl From<&String> for Value {
    fn from(s: &String) -> Self {
        Value::String(s.clone())
    }
}
impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Value::Integer(i)
    }
}
impl From<i32> for Value {
    fn from(i: i32) -> Self {
        Value::Integer(i64::from(i))
    }
}
impl From<u32> for Value {
    fn from(i: u32) -> Self {
        Value::Integer(i64::from(i))
    }
}
impl From<u64> for Value {
    fn from(i: u64) -> Self {
        Value::Unsigned(i)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Value::Unsigned(i as u64)
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}
impl From<f64> for Value {
    fn from(x: f64) -> Self {
        Value::Float(x)
    }
}
impl From<f32> for Value {
    fn from(x: f32) -> Self {
        Value::Float(f64::from(x))
    }
}
