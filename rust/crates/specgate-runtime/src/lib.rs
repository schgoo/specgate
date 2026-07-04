//! `SpecGate` runtime — thread-local trace buffer + mock table + `SpecEvent` /
//! `ToSpecValue` traits + structured `Value` type + operation registry.
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
use std::fmt::Write as _;

pub use linkme;

// ---------------------------------------------------------------------------
// Operation registry — populated at link time via #[distributed_slice].
// The harness discovery binary iterates this to find all annotated operations.
// ---------------------------------------------------------------------------

/// Metadata about one annotated operation or setup.
#[derive(Debug, Clone)]
pub struct OpMeta {
    pub name: &'static str,
    pub module_path: &'static str,
    pub fn_name: &'static str,
    pub is_setup: bool,
    pub is_async: bool,
    pub params: &'static [(&'static str, &'static str)],
    pub return_type: &'static str,
    /// For setups: the operation parameter this setup fills (empty if unset).
    /// Used to disambiguate when several params share the setup's output type.
    pub fills: &'static str,
    /// The component (declared via `spec_component!` or a per-item `spec = "…"`
    /// override) that owns this operation. Extraction groups by component and
    /// derives cross-component `depends_on` from it.
    pub component: &'static str,
}

/// One named field with its (stringified) Rust type. Used for both operation
/// parameters and `SpecEvent` struct/enum-variant fields.
pub type FieldMeta = (&'static str, &'static str);

/// One enum variant: its name plus any named fields. Tuple and unit variants
/// carry an empty field list (schema extraction maps them to `{}`).
#[derive(Debug, Clone)]
pub struct VariantMeta {
    pub name: &'static str,
    pub fields: &'static [FieldMeta],
}

/// Metadata about a struct/enum that derives `SpecEvent`. `kind` is `"struct"`
/// or `"enum"`. Structs populate `fields` (only `#[spec_event]`-tagged fields,
/// honoring `#[spec_event(name = "…")]`); enums populate `variants`.
#[derive(Debug, Clone)]
pub struct TypeMeta {
    pub name: &'static str,
    pub module_path: &'static str,
    pub kind: &'static str,
    pub fields: &'static [FieldMeta],
    pub variants: &'static [VariantMeta],
    /// The component that owns this type (see `OpMeta::component`).
    pub component: &'static str,
}

#[linkme::distributed_slice]
pub static SPECGATE_OPS: [OpMeta];

#[linkme::distributed_slice]
pub static SPECGATE_TYPES: [TypeMeta];

/// Escape a string for inclusion as a JSON string literal. Handles the control
/// and structural characters that can appear in stringified Rust types (quotes,
/// backslashes); other characters pass through. Kept dependency-free so the
/// runtime stays lean.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Write a JSON array of `[name, type]` field pairs into `out`.
fn write_fields_json(out: &mut String, fields: &[FieldMeta]) {
    out.push('[');
    for (i, (name, ty)) in fields.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[\"{}\",\"{}\"]", json_escape(name), json_escape(ty));
    }
    out.push(']');
}

/// Collect all registered metadata as JSON (used by the discovery binary).
#[must_use]
pub fn discovery_json() -> String {
    let mut out = String::from("{\"operations\":[");
    for (i, op) in SPECGATE_OPS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"name\":\"{}\",\"module_path\":\"{}\",\"fn_name\":\"{}\",\"is_setup\":{},\"is_async\":{},\"return_type\":\"{}\",\"fills\":\"{}\",\"component\":\"{}\",\"params\":",
            json_escape(op.name),
            json_escape(op.module_path),
            json_escape(op.fn_name),
            op.is_setup,
            op.is_async,
            json_escape(op.return_type),
            json_escape(op.fills),
            json_escape(op.component),
        );
        write_fields_json(&mut out, op.params);
        out.push('}');
    }
    out.push_str("],\"types\":[");
    for (i, ty) in SPECGATE_TYPES.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"name\":\"{}\",\"module_path\":\"{}\",\"kind\":\"{}\",\"component\":\"{}\",\"fields\":",
            json_escape(ty.name),
            json_escape(ty.module_path),
            json_escape(ty.kind),
            json_escape(ty.component),
        );
        write_fields_json(&mut out, ty.fields);
        out.push_str(",\"variants\":[");
        for (j, v) in ty.variants.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            let _ = write!(out, "{{\"name\":\"{}\",\"fields\":", json_escape(v.name));
            write_fields_json(&mut out, v.fields);
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

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
    #[must_use]
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
    // i64 → f64 is intentionally lossy: comparing an integer variant against a float
    // variant uses float semantics, which cannot be made lossless for large i64 values.
    #[allow(clippy::cast_precision_loss)]
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
            Value::String(s) => write!(f, "{s}"),
            Value::Integer(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::Bool(b) => write!(f, "{b}"),
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
                    write!(f, "\"{k}\":")?;
                    write_display_atom(f, v)?;
                }
                write!(f, "}}")
            }
        }
    }
}

fn write_display_atom(f: &mut std::fmt::Formatter<'_>, v: &Value) -> std::fmt::Result {
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
impl From<usize> for Value {
    #[allow(clippy::cast_possible_wrap)] // usize to i64: may wrap for values > i64::MAX on 64-bit; not expected in spec traces
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
        Value::Float(f64::from(x))
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
    #[allow(clippy::cast_possible_wrap)] // u64 YAML integers may exceed i64::MAX; wrap accepted for spec trace values
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
    #[must_use]
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
    let event = TraceEvent::Event {
        name: name.to_string(),
        value,
    };
    record_event(&event);
    BUFFER.with(|b| {
        b.borrow_mut().push(event);
    });
}

pub fn emit_run(operation: &str) {
    let event = TraceEvent::Run {
        operation: operation.to_string(),
    };
    record_event(&event);
    BUFFER.with(|b| {
        b.borrow_mut().push(event);
    });
}

/// Record-mode sink. When the `SPECGATE_RECORD` environment variable names a
/// (non-empty) file, every emitted event is also appended to that file as one
/// JSON object per line (JSONL of [`TraceEvent`], which is `#[serde(tag =
/// "kind")]`). The `extract --cases` command sets this while running a target
/// crate's tests so the events a plain `#[test]` produces can be captured and
/// turned into spec cases. The buffer path is unaffected; this is purely an
/// additional side channel that is a no-op when the variable is unset.
fn record_event(event: &TraceEvent) {
    let Ok(path) = std::env::var("SPECGATE_RECORD") else {
        return;
    };
    if path.is_empty() {
        return;
    }
    append_record_line(&path, event);
}

/// Append one JSONL-serialized event to the record file at `path`. Failures
/// (unwritable path, serialization error) are silently ignored so record mode
/// never perturbs the traced program.
fn append_record_line(path: &str, event: &TraceEvent) {
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path)
        && let Ok(line) = serde_json::to_string(event)
    {
        use std::io::Write as _;
        let _ = writeln!(file, "{line}");
    }
}

#[must_use]
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

#[must_use]
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

/// Marker implemented (via `#[derive(SpecEvent)]`) ONLY by struct types, never
/// enums. It lets return-value emission distinguish a struct return (which emits
/// both its per-field events and a structured `$result`) from an enum return
/// (which emits only the tagged `$result`). See [`ReturnEmit`].
pub trait SpecEventStruct: SpecEvent + ToSpecValue {}

// ---------------------------------------------------------------------------
// ToSpecValue — convert any annotated value to a structured `Value`.
// ---------------------------------------------------------------------------

pub trait ToSpecValue {
    fn to_spec_value(&self) -> Value;
}

macro_rules! to_spec_value_int {
    ($($t:ty),*) => {
        $(impl ToSpecValue for $t {
            fn to_spec_value(&self) -> Value { Value::Integer(i64::from(*self)) }
        })*
    };
}
to_spec_value_int!(i8, i16, i32, u8, u16, u32);

impl ToSpecValue for isize {
    fn to_spec_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToSpecValue for u64 {
    #[allow(clippy::cast_possible_wrap)] // u64 values > i64::MAX are not expected in spec traces; wrap accepted
    fn to_spec_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToSpecValue for usize {
    #[allow(clippy::cast_possible_wrap)] // usize to i64 on 64-bit; values > i64::MAX are not expected in spec traces
    fn to_spec_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToSpecValue for i64 {
    fn to_spec_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl ToSpecValue for f32 {
    fn to_spec_value(&self) -> Value {
        Value::Float(f64::from(*self))
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
impl<T: ToSpecValue, S: std::hash::BuildHasher> ToSpecValue for HashMap<String, T, S> {
    fn to_spec_value(&self) -> Value {
        Value::Map(self.iter().map(|(k, v)| (k.clone(), v.to_spec_value())).collect())
    }
}
impl<T: ToSpecValue + Ord> ToSpecValue for BTreeSet<T> {
    fn to_spec_value(&self) -> Value {
        Value::Set(self.iter().map(ToSpecValue::to_spec_value).collect())
    }
}
impl<T: ToSpecValue + Eq + std::hash::Hash, S: std::hash::BuildHasher> ToSpecValue for HashSet<T, S> {
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

impl ToSpecValue for Value {
    fn to_spec_value(&self) -> Value {
        self.clone()
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
// ReturnEmit — autoref specialization that emits an operation's `$result` (and,
// for struct returns, its per-field events) based on the return value's type.
// The macro-expanded body of `#[spec_operation]` for a non-scalar, non-Result,
// non-Option return ends with `(&&&#rt::ReturnEmit(&__sg_ret)).emit_result();`.
//
// Resolution ladder (highest → lowest priority — MORE `&` on the impl Self type
// is tried first: the macro calls this with four references, so method lookup
// matches the most-referenced by-value receiver at the outermost step before
// dereferencing to the lower-priority levels):
//   1. `T: SpecEventStruct` (struct)            → emit_fields + structured $result
//   2. `T: ToSpecValue` (enum / collection)     → structured $result only
//   3. `T: Display` (any other printable value) → Display-string $result
//   4. (no bound) any other return type         → emits nothing
//
// All four are TRAIT impls (never an inherent method) so that an unsatisfied
// bound falls through to the next level instead of hard-erroring. The Level 4
// universal fallback ensures an annotated op whose return type implements none
// of the higher traits (e.g. a non-SpecEvent struct that `extract` will later
// reject as an "unresolved type") still COMPILES, emitting no `$result`.
//
// Scalars (i32/String/&str/bool/…) are handled by the macro directly via the
// Display path and never reach this ladder, so a primitive's `ToSpecValue`
// impl does not shadow its intended Display formatting.
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ReturnEmit<'a, T: ?Sized>(pub &'a T);

// Level 1 (highest priority) — struct returns: per-field events + $result.
pub trait ReturnEmitStruct {
    fn emit_result(&self);
}

impl<T: SpecEventStruct + ?Sized> ReturnEmitStruct for &&&ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self) {
        self.0.emit_fields(None);
        emit_event_v("$result", self.0.to_spec_value());
    }
}

// Level 2 — enums / collections / any `ToSpecValue`: structured $result only.
pub trait ReturnEmitToSpec {
    fn emit_result(&self);
}

impl<T: ToSpecValue + ?Sized> ReturnEmitToSpec for &&ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self) {
        emit_event_v("$result", self.0.to_spec_value());
    }
}

// Level 3 — any `Display` value: Display-string $result.
pub trait ReturnEmitDisplay {
    fn emit_result(&self);
}

impl<T: std::fmt::Display + ?Sized> ReturnEmitDisplay for &ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self) {
        emit_event_v("$result", Value::String(format!("{}", self.0)));
    }
}

// Level 4 (lowest priority, universal fallback) — any return type: emits nothing.
pub trait ReturnEmitNone {
    fn emit_result(&self);
}

impl<T: ?Sized> ReturnEmitNone for ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self) {}
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn trace_event_jsonl_roundtrips() {
        // Record mode persists events as JSONL; each line must round-trip back
        // into the same TraceEvent (the capture driver parses these).
        let events = vec![
            TraceEvent::Run { operation: "add".into() },
            TraceEvent::Event {
                name: "add.a".into(),
                value: Value::String("2".into()),
            },
            TraceEvent::Event {
                name: "$result".into(),
                value: Value::String("5".into()),
            },
        ];
        for ev in &events {
            let line = serde_json::to_string(ev).unwrap();
            let back: TraceEvent = serde_json::from_str(&line).unwrap();
            assert_eq!(&back, ev);
        }
        // The tag discriminates the two shapes.
        let run_line = serde_json::to_string(&events[0]).unwrap();
        assert!(run_line.contains("\"kind\":\"Run\""), "{run_line}");
    }

    #[test]
    fn append_record_line_writes_parseable_jsonl() {
        // Build artifacts belong under the workspace target dir (gitignored).
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("target");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("specgate-record-test-{}.jsonl", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let path_str = path.to_string_lossy().to_string();

        let e1 = TraceEvent::Run { operation: "greet".into() };
        let e2 = TraceEvent::Event {
            name: "greet.name".into(),
            value: Value::String("world".into()),
        };
        append_record_line(&path_str, &e1);
        append_record_line(&path_str, &e2);

        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<TraceEvent> = text.lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(parsed, vec![e1, e2]);

        let _ = std::fs::remove_file(&path);
    }
}
