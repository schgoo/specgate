//! SpecGate runtime — thread-local trace buffer + mock table + SpecEvent /
//! ToSpecValue traits + structured `Value` type.
//!
//! Companion to the `specgate-annotations` proc-macro crate. The macros
//! expand into calls into this runtime; user code never references this
//! crate directly.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

// ---------------------------------------------------------------------------
// Value — structured trace event payload.
// ---------------------------------------------------------------------------

/// Structured trace value. Scalars round-trip directly; collections preserve
/// their shape so matchers can apply size / contains / etc. checks.
#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    List(Vec<Value>),
    Map(BTreeMap<String, Value>),
    Set(BTreeSet<Value>),
}

impl Value {
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::String(_) => "string",
            Value::Integer(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Map(_) => "map",
            Value::Set(_) => "set",
        }
    }
}

fn variant_rank(v: &Value) -> u8 {
    match v {
        Value::Bool(_) => 0,
        Value::Integer(_) => 1,
        Value::Float(_) => 2,
        Value::String(_) => 3,
        Value::List(_) => 4,
        Value::Set(_) => 5,
        Value::Map(_) => 6,
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Integer(a), Value::Integer(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a.to_bits() == b.to_bits(),
            (Value::Integer(a), Value::Float(b)) | (Value::Float(b), Value::Integer(a)) => (*a as f64).to_bits() == b.to_bits(),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::List(a), Value::List(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Set(a), Value::Set(b)) => a == b,
            // Treat List and Set as equal if their contents match as sets.
            (Value::List(a), Value::Set(b)) | (Value::Set(b), Value::List(a)) => a.len() == b.len() && a.iter().all(|x| b.contains(x)),
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
            Value::String(s) => write!(f, "{}", s),
            Value::Integer(i) => write!(f, "{}", i),
            Value::Float(x) => write!(f, "{}", x),
            Value::Bool(b) => write!(f, "{}", b),
            Value::List(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_display_atom(f, v)?;
                }
                write!(f, "]")
            }
            Value::Set(items) => {
                write!(f, "[")?;
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write_display_atom(f, v)?;
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ",")?;
                    }
                    write!(f, "\"{}\":", k)?;
                    write_display_atom(f, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_display_atom(f: &mut std::fmt::Formatter<'_>, v: &Value) -> std::fmt::Result {
    match v {
        Value::String(s) => write!(f, "\"{}\"", s),
        other => write!(f, "{}", other),
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
        Value::Integer(i as i64)
    }
}
impl From<u32> for Value {
    fn from(i: u32) -> Self {
        Value::Integer(i as i64)
    }
}
impl From<usize> for Value {
    fn from(i: usize) -> Self {
        Value::Integer(i as i64)
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
        Value::Float(x as f64)
    }
}

// --- Serialize ------------------------------------------------------------

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::String(v) => s.serialize_str(v),
            Value::Integer(v) => s.serialize_i64(*v),
            Value::Float(v) => s.serialize_f64(*v),
            Value::Bool(v) => s.serialize_bool(*v),
            Value::List(items) => {
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for it in items {
                    seq.serialize_element(it)?;
                }
                seq.end()
            }
            Value::Set(items) => {
                // Sets are emitted as ordered arrays; round-trip turns them
                // back into Value::List, which the matcher treats fungibly.
                let mut seq = s.serialize_seq(Some(items.len()))?;
                for it in items {
                    seq.serialize_element(it)?;
                }
                seq.end()
            }
            Value::Map(map) => {
                let mut m = s.serialize_map(Some(map.len()))?;
                for (k, v) in map {
                    m.serialize_entry(k, v)?;
                }
                m.end()
            }
        }
    }
}

// --- Deserialize ----------------------------------------------------------

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        d.deserialize_any(ValueVisitor)
    }
}

struct ValueVisitor;
impl<'de> Visitor<'de> for ValueVisitor {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("any JSON/YAML value")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(Value::Integer(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Value::Integer(v as i64))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        Ok(Value::Float(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Value::String(v.to_string()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::String(v))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::String(String::new()))
    }
    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::String(String::new()))
    }
    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        Deserialize::deserialize(d)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = seq.next_element()? {
            out.push(v);
        }
        Ok(Value::List(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut out = BTreeMap::new();
        while let Some((k, v)) = map.next_entry::<String, Value>()? {
            out.insert(k, v);
        }
        Ok(Value::Map(out))
    }
}

// ---------------------------------------------------------------------------
// TraceEvent.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TraceEvent {
    Event { name: String, value: Value },
    Run { operation: String },
}

impl TraceEvent {
    pub fn name(&self) -> String {
        match self {
            TraceEvent::Event { name, .. } => name.clone(),
            TraceEvent::Run { operation } => operation.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Thread-local trace buffer + mock table.
// ---------------------------------------------------------------------------

thread_local! {
    static BUFFER: RefCell<Vec<TraceEvent>> = const { RefCell::new(Vec::new()) };
    static MOCKS: RefCell<HashMap<String, HashMap<String, String>>> =
        RefCell::new(HashMap::new());
}

/// Push an `Event { name, value }` onto the thread-local trace buffer. The
/// `&str`-taking shim is preserved so existing macro expansions and call
/// sites that pass `format!("{}", x)` keep compiling unchanged.
pub fn emit_event(name: &str, value: &str) {
    emit_event_v(name, Value::String(value.to_string()));
}

/// Push a structured `Event { name, value }`.
pub fn emit_event_v(name: &str, value: Value) {
    BUFFER.with(|b| {
        b.borrow_mut().push(TraceEvent::Event {
            name: name.to_string(),
            value,
        })
    });
}

pub fn emit_run(operation: &str) {
    BUFFER.with(|b| {
        b.borrow_mut().push(TraceEvent::Run {
            operation: operation.to_string(),
        })
    });
}

pub fn take_traces() -> Vec<TraceEvent> {
    BUFFER.with(|b| std::mem::take(&mut *b.borrow_mut()))
}

pub fn reset() {
    BUFFER.with(|b| b.borrow_mut().clear());
    MOCKS.with(|m| m.borrow_mut().clear());
}

pub fn set_mock(mock_name: &str, entries: &[(&str, &str)]) {
    let mut map = HashMap::new();
    for (k, v) in entries {
        map.insert((*k).to_string(), (*v).to_string());
    }
    MOCKS.with(|m| {
        m.borrow_mut().insert(mock_name.to_string(), map);
    });
}

pub fn mock_lookup(mock_name: &str, input: &str) -> Option<String> {
    MOCKS.with(|m| m.borrow().get(mock_name).and_then(|t| t.get(input).cloned()))
}

// ---------------------------------------------------------------------------
// SpecEvent — implemented (typically via `#[derive(SpecEvent)]`) by structs
// that expose annotated fields.
// ---------------------------------------------------------------------------

pub trait SpecEvent {
    fn emit_fields(&self, prefix: Option<&str>);
}

// ---------------------------------------------------------------------------
// ToSpecValue — convert any annotated value to a structured `Value`.
// ---------------------------------------------------------------------------

pub trait ToSpecValue {
    fn to_spec_value(&self) -> Value;
}

macro_rules! to_spec_value_int {
    ($($t:ty),*) => {
        $(impl ToSpecValue for $t {
            fn to_spec_value(&self) -> Value { Value::Integer(*self as i64) }
        })*
    };
}
to_spec_value_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl ToSpecValue for f32 {
    fn to_spec_value(&self) -> Value {
        Value::Float(*self as f64)
    }
}
impl ToSpecValue for f64 {
    fn to_spec_value(&self) -> Value {
        Value::Float(*self)
    }
}
impl ToSpecValue for bool {
    fn to_spec_value(&self) -> Value {
        Value::Bool(*self)
    }
}
impl ToSpecValue for char {
    fn to_spec_value(&self) -> Value {
        Value::String(self.to_string())
    }
}
impl ToSpecValue for str {
    fn to_spec_value(&self) -> Value {
        Value::String(self.to_string())
    }
}
impl ToSpecValue for String {
    fn to_spec_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl<T: ToSpecValue> ToSpecValue for Vec<T> {
    fn to_spec_value(&self) -> Value {
        Value::List(self.iter().map(ToSpecValue::to_spec_value).collect())
    }
}
impl<T: ToSpecValue> ToSpecValue for [T] {
    fn to_spec_value(&self) -> Value {
        Value::List(self.iter().map(ToSpecValue::to_spec_value).collect())
    }
}
impl<T: ToSpecValue, const N: usize> ToSpecValue for [T; N] {
    fn to_spec_value(&self) -> Value {
        Value::List(self.iter().map(ToSpecValue::to_spec_value).collect())
    }
}
impl<T: ToSpecValue> ToSpecValue for BTreeMap<String, T> {
    fn to_spec_value(&self) -> Value {
        Value::Map(self.iter().map(|(k, v)| (k.clone(), v.to_spec_value())).collect())
    }
}
impl<T: ToSpecValue> ToSpecValue for HashMap<String, T> {
    fn to_spec_value(&self) -> Value {
        Value::Map(self.iter().map(|(k, v)| (k.clone(), v.to_spec_value())).collect())
    }
}
impl<T: ToSpecValue + Ord> ToSpecValue for BTreeSet<T> {
    fn to_spec_value(&self) -> Value {
        Value::Set(self.iter().map(ToSpecValue::to_spec_value).collect())
    }
}
impl<T: ToSpecValue + Eq + std::hash::Hash> ToSpecValue for HashSet<T> {
    fn to_spec_value(&self) -> Value {
        let mut v: Vec<Value> = self.iter().map(ToSpecValue::to_spec_value).collect();
        v.sort();
        Value::Set(v.into_iter().collect())
    }
}

impl<T: ToSpecValue> ToSpecValue for Option<T> {
    fn to_spec_value(&self) -> Value {
        match self {
            Some(v) => v.to_spec_value(),
            None => Value::String(String::new()),
        }
    }
}

impl<T: ToSpecValue + ?Sized> ToSpecValue for &T {
    fn to_spec_value(&self) -> Value {
        (**self).to_spec_value()
    }
}
impl<T: ToSpecValue + ?Sized> ToSpecValue for Box<T> {
    fn to_spec_value(&self) -> Value {
        (**self).to_spec_value()
    }
}

// ---------------------------------------------------------------------------
// ReturnEmit — autoref specialization that picks `emit_fields` for SpecEvent
// types and `to_spec_value` for everything else. The macro-expanded body of
// `#[spec_operation]` ends with `(&ReturnEmit(&__sg_ret)).emit("$result");`.
// ---------------------------------------------------------------------------

pub struct ReturnEmit<'a, T: ?Sized>(pub &'a T);

// More-specific inherent impl: chosen first by method lookup when `T: SpecEvent`.
impl<T: SpecEvent + ?Sized> ReturnEmit<'_, T> {
    #[inline]
    pub fn emit(&self, _name: &str) {
        self.0.emit_fields(None);
    }
}

// Fallback via trait (visible through autoref).
pub trait ReturnEmitFallback {
    fn emit(&self, name: &str);
}

impl<T: ToSpecValue + ?Sized> ReturnEmitFallback for &ReturnEmit<'_, T> {
    #[inline]
    fn emit(&self, name: &str) {
        emit_event_v(name, self.0.to_spec_value());
    }
}

/// Least-specific fallback — any `T: Display` is emitted as a String value.
pub trait ReturnEmitDisplay {
    fn emit(&self, name: &str);
}

impl<T: std::fmt::Display + ?Sized> ReturnEmitDisplay for &&ReturnEmit<'_, T> {
    #[inline]
    fn emit(&self, name: &str) {
        emit_event_v(name, Value::String(format!("{}", self.0)));
    }
}
