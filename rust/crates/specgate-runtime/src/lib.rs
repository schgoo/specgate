//! `SpecGate` runtime — the support library the annotation macros expand into.
//!
//! Provides native structured operation capture plus the temporary flat trace
//! buffer still used by extraction and the spec harness, the mock table, the
//! `SpecEvent` / value-projection traits, the structured `Value` type, and the
//! link-time operation/type registry that `specgate extract` reads.
//! Isolated test processes can activate native capture through
//! `SPECGATE_NATIVE_CAPTURE`; the first operation starts the session lazily and
//! each completed top-level operation atomically refreshes a stable JSON
//! sidecar containing the full scenario.
//!
//! Companion to the `specgate-annotations` proc-macro crate: the macros expand
//! into calls into this runtime, so user code never references it directly.

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use linkme;

// ---------------------------------------------------------------------------
// Operation registry — populated at link time via #[distributed_slice].
// The harness discovery binary iterates this to find all annotated operations.
// ---------------------------------------------------------------------------

/// Metadata about one annotated operation or setup.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct OpMeta {
    pub name: &'static str,
    pub module_path: &'static str,
    pub fn_name: &'static str,
    pub is_setup: bool,
    pub is_async: bool,
    pub is_method: bool,
    pub is_public: bool,
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
            "{{\"name\":\"{}\",\"module_path\":\"{}\",\"fn_name\":\"{}\",\"is_setup\":{},\"is_async\":{},\"is_method\":{},\"is_public\":{},\"return_type\":\"{}\",\"fills\":\"{}\",\"component\":\"{}\",\"params\":",
            json_escape(op.name),
            json_escape(op.module_path),
            json_escape(op.fn_name),
            op.is_setup,
            op.is_async,
            op.is_method,
            op.is_public,
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
// Native synchronous operation capture.
// ---------------------------------------------------------------------------

/// Deterministic configuration for one thread-local native capture session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCaptureConfig {
    pub scenario_name: String,
    pub trace_id: String,
    pub run_span_id: String,
    pub scenario_span_id: String,
    pub operation_span_ids: Vec<String>,
    pub start_time_unix_nano: i64,
    pub clock_step_unix_nano: i64,
}

/// Private-process activation payload for environment-driven native capture.
///
/// The CLI serializes this value into `SPECGATE_NATIVE_CAPTURE`; the first
/// annotated synchronous operation in the isolated test process starts the
/// session lazily and persists snapshots to `sidecar_path`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCaptureEnvironmentConfig {
    pub capture: NativeCaptureConfig,
    pub sidecar_path: PathBuf,
}

/// A completed run or scenario span boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeSpanBoundary {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub status: NativeStatus,
}

/// Terminal status for a native span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeStatus {
    Ok,
    Error,
}

/// One native observation captured while an operation scope is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeObservation {
    pub order: u64,
    pub time_unix_nano: i64,
    pub name: String,
    pub value: Value,
}

/// The semantic completion recorded at an operation's actual return boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NativeCompletion {
    Result {
        order: u64,
        time_unix_nano: i64,
        value: Value,
    },
    Fault {
        order: u64,
        time_unix_nano: i64,
        fault_type: String,
        message: String,
        observer: String,
    },
}

/// One completed native operation span.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeOperationSpan {
    pub order: u64,
    pub span_id: String,
    pub parent_span_id: String,
    pub component_id: String,
    pub operation_name: String,
    pub start_time_unix_nano: i64,
    pub end_time_unix_nano: i64,
    pub status: NativeStatus,
    pub inputs: BTreeMap<String, Value>,
    pub observations: Vec<NativeObservation>,
    pub completion: Option<NativeCompletion>,
}

/// Completed native evidence for one deterministic run and scenario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeCapture {
    pub trace_id: String,
    pub scenario_name: String,
    pub run: NativeSpanBoundary,
    pub scenario: NativeSpanBoundary,
    pub operations: Vec<NativeOperationSpan>,
}

#[derive(Debug)]
struct PendingNativeOperation {
    order: u64,
    span_id: String,
    parent_span_id: String,
    component_id: String,
    operation_name: String,
    start_time_unix_nano: i64,
    end_time_unix_nano: Option<i64>,
    status: Option<NativeStatus>,
    inputs: BTreeMap<String, Value>,
    observations: Vec<NativeObservation>,
    completion: Option<NativeCompletion>,
}

#[derive(Debug)]
struct NativeCaptureState {
    config: NativeCaptureConfig,
    sidecar_path: Option<PathBuf>,
    run_start_time_unix_nano: i64,
    scenario_start_time_unix_nano: i64,
    next_time_unix_nano: i64,
    next_operation_id: usize,
    next_order: u64,
    active_operations: Vec<usize>,
    operations: Vec<PendingNativeOperation>,
    terminal_error: Option<String>,
}

impl NativeCaptureState {
    fn tick(&mut self) -> Result<i64, String> {
        let current = self.next_time_unix_nano;
        self.next_time_unix_nano = current
            .checked_add(self.config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        Ok(current)
    }

    fn order(&mut self) -> Result<u64, String> {
        let current = self.next_order;
        self.next_order = current
            .checked_add(1)
            .ok_or_else(|| "native capture event order overflow".to_string())?;
        Ok(current)
    }
}

/// RAII guard for one annotation-generated synchronous operation invocation.
///
/// The inactive representation is allocation-free and is returned whenever no
/// native capture session is active.
#[derive(Debug)]
pub struct OperationScope {
    operation_index: Option<usize>,
    closed: bool,
}

impl OperationScope {
    #[must_use]
    pub const fn inactive() -> Self {
        Self {
            operation_index: None,
            closed: true,
        }
    }

    /// Record one semantic input without adding a native observation.
    ///
    /// # Errors
    ///
    /// Returns an error for an out-of-order scope, a duplicate input, or a
    /// scope that has already completed.
    pub fn record_input(&mut self, name: &str, value: Value) -> Result<(), String> {
        let Some(operation_index) = self.operation_index else {
            return Ok(());
        };
        with_native_state_mut(|state| {
            ensure_active_operation(state, operation_index)?;
            let operation = &mut state.operations[operation_index];
            if operation.status.is_some() {
                return Err(format!("native operation '{}' is already complete", operation.operation_name));
            }
            if operation.inputs.insert(name.to_string(), value).is_some() {
                return Err(format!(
                    "native operation '{}' input '{name}' was recorded twice",
                    operation.operation_name
                ));
            }
            Ok(())
        })
    }

    /// Complete this operation with a typed semantic result.
    ///
    /// # Errors
    ///
    /// Returns an error for double completion or non-LIFO completion.
    pub fn complete_result(&mut self, value: Value) -> Result<(), String> {
        self.complete(Some(value))
    }

    /// Complete this operation successfully without a completion event.
    ///
    /// # Errors
    ///
    /// Returns an error for double completion or non-LIFO completion.
    pub fn complete_unit(&mut self) -> Result<(), String> {
        self.complete(None)
    }

    fn complete(&mut self, result: Option<Value>) -> Result<(), String> {
        let Some(operation_index) = self.operation_index else {
            return Ok(());
        };
        let should_persist = with_native_state_mut(|state| {
            ensure_active_operation(state, operation_index)?;
            if state.operations[operation_index].status.is_some() {
                return Err(format!(
                    "native operation '{}' was completed twice",
                    state.operations[operation_index].operation_name
                ));
            }
            let completion = if let Some(value) = result {
                Some(NativeCompletion::Result {
                    order: state.order()?,
                    time_unix_nano: state.tick()?,
                    value,
                })
            } else {
                None
            };
            let end_time_unix_nano = state.tick()?;
            let operation = &mut state.operations[operation_index];
            operation.completion = completion;
            operation.end_time_unix_nano = Some(end_time_unix_nano);
            operation.status = Some(NativeStatus::Ok);
            state.active_operations.pop();
            Ok(state.active_operations.is_empty() && state.sidecar_path.is_some())
        })?;
        self.closed = true;
        if should_persist {
            persist_active_native_capture()?;
        }
        Ok(())
    }
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let Some(operation_index) = self.operation_index else {
            return;
        };
        let panicking = std::thread::panicking();
        let result = with_native_state_mut(|state| {
            if state
                .operations
                .get(operation_index)
                .and_then(|operation| operation.status)
                .is_some()
            {
                return Ok(false);
            }
            ensure_active_operation(state, operation_index)?;
            if panicking {
                let completion = NativeCompletion::Fault {
                    order: state.order()?,
                    time_unix_nano: state.tick()?,
                    fault_type: "specgate.unexpected_target_fault".to_string(),
                    message: "operation unwound before returning".to_string(),
                    observer: "target".to_string(),
                };
                let end_time_unix_nano = state.tick()?;
                let operation = &mut state.operations[operation_index];
                operation.completion = Some(completion);
                operation.end_time_unix_nano = Some(end_time_unix_nano);
                operation.status = Some(NativeStatus::Error);
                state.active_operations.pop();
                Ok(state.active_operations.is_empty() && state.sidecar_path.is_some())
            } else {
                let message = format!(
                    "native operation '{}' scope closed without completion",
                    state.operations[operation_index].operation_name
                );
                state.terminal_error = Some(message.clone());
                state.active_operations.pop();
                Err(message)
            }
        });
        match result {
            Ok(true) => {
                if let Err(error) = persist_active_native_capture() {
                    if panicking {
                        eprintln!("failed to persist native capture during unwind: {error}");
                    } else {
                        panic!("failed to persist native capture: {error}");
                    }
                }
            }
            Ok(false) => {}
            Err(error) if !panicking => panic!("{error}"),
            Err(error) => eprintln!("native capture failed during unwind: {error}"),
        }
    }
}

/// Start one deterministic thread-local native capture session.
///
/// # Errors
///
/// Rejects malformed or duplicate identifiers, non-positive clock settings,
/// and attempts to replace an active session.
pub fn start_native_capture(config: NativeCaptureConfig) -> Result<(), String> {
    start_native_capture_with_sidecar(config, None)
}

fn start_native_capture_with_sidecar(config: NativeCaptureConfig, sidecar_path: Option<PathBuf>) -> Result<(), String> {
    validate_native_capture_config(&config)?;
    if sidecar_path.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
        return Err("native capture sidecar path must not be empty".to_string());
    }
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return Err("a native capture session is already active".to_string());
        }
        let scenario_start_time_unix_nano = config
            .start_time_unix_nano
            .checked_add(config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let next_time_unix_nano = scenario_start_time_unix_nano
            .checked_add(config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        *slot = Some(NativeCaptureState {
            run_start_time_unix_nano: config.start_time_unix_nano,
            scenario_start_time_unix_nano,
            next_time_unix_nano,
            config,
            sidecar_path,
            next_operation_id: 0,
            next_order: 0,
            active_operations: Vec::new(),
            operations: Vec::new(),
            terminal_error: None,
        });
        Ok(())
    })
}

/// Begin a native operation scope, or return a cheap inactive guard.
///
/// # Errors
///
/// Returns an error when the deterministic operation ID list is exhausted or
/// the logical clock cannot advance.
pub fn begin_native_operation(component_id: &str, operation_name: &str) -> Result<OperationScope, String> {
    activate_native_capture_from_environment()?;
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(OperationScope::inactive());
        };
        if let Some(error) = &state.terminal_error {
            return Err(error.clone());
        }
        let span_id = next_operation_span_id(state)?;
        let parent_span_id = state.active_operations.last().map_or_else(
            || state.config.scenario_span_id.clone(),
            |index| state.operations[*index].span_id.clone(),
        );
        let start_time_unix_nano = state.tick()?;
        let order = state.order()?;
        let operation_index = state.operations.len();
        state.operations.push(PendingNativeOperation {
            order,
            span_id,
            parent_span_id,
            component_id: component_id.to_string(),
            operation_name: operation_name.to_string(),
            start_time_unix_nano,
            end_time_unix_nano: None,
            status: None,
            inputs: BTreeMap::new(),
            observations: Vec::new(),
            completion: None,
        });
        state.next_operation_id += 1;
        state.active_operations.push(operation_index);
        Ok(OperationScope {
            operation_index: Some(operation_index),
            closed: false,
        })
    })
}

/// Finish and take the active native capture.
///
/// # Errors
///
/// Rejects absent sessions, nested/unclosed scopes, prior scope errors, unused
/// deterministic operation IDs, and logical clock overflow.
pub fn finish_native_capture() -> Result<NativeCapture, String> {
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.take() else {
            return Err("no native capture session is active".to_string());
        };
        if !state.active_operations.is_empty() {
            let names = state
                .active_operations
                .iter()
                .map(|index| state.operations[*index].operation_name.as_str())
                .collect::<Vec<_>>()
                .join(" -> ");
            *slot = Some(state);
            return Err(format!("native capture has nested/unclosed operation scopes: {names}"));
        }
        if let Some(error) = state.terminal_error {
            return Err(error);
        }
        if state.next_operation_id < state.config.operation_span_ids.len() {
            return Err(format!(
                "native capture supplied {} operation span IDs but consumed {}",
                state.config.operation_span_ids.len(),
                state.next_operation_id
            ));
        }
        build_native_capture(&state)
    })
}

fn activate_native_capture_from_environment() -> Result<(), String> {
    if NATIVE_CAPTURE.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let Some(encoded) = std::env::var_os("SPECGATE_NATIVE_CAPTURE") else {
        return Ok(());
    };
    if encoded.is_empty() {
        return Ok(());
    }
    let environment: NativeCaptureEnvironmentConfig = serde_json::from_str(&encoded.to_string_lossy())
        .map_err(|error| format!("invalid SPECGATE_NATIVE_CAPTURE configuration: {error}"))?;
    start_native_capture_with_sidecar(environment.capture, Some(environment.sidecar_path))
}

fn next_operation_span_id(state: &NativeCaptureState) -> Result<String, String> {
    if let Some(span_id) = state.config.operation_span_ids.get(state.next_operation_id) {
        return Ok(span_id.clone());
    }

    let mut candidate = u64::try_from(state.next_operation_id)
        .map_err(|_error| "native operation span ID sequence overflow".to_string())?
        .checked_add(1)
        .ok_or_else(|| "native operation span ID sequence overflow".to_string())?;
    loop {
        let span_id = format!("{candidate:016x}");
        let is_reserved = span_id == state.config.run_span_id
            || span_id == state.config.scenario_span_id
            || state.operations.iter().any(|operation| operation.span_id == span_id);
        if !is_reserved {
            return Ok(span_id);
        }
        candidate = candidate
            .checked_add(1)
            .ok_or_else(|| "native operation span ID sequence overflow".to_string())?;
    }
}

fn persist_active_native_capture() -> Result<(), String> {
    let (path, capture) = NATIVE_CAPTURE.with(|slot| {
        let slot = slot.borrow();
        let state = slot
            .as_ref()
            .ok_or_else(|| "native capture session ended before snapshot persistence".to_string())?;
        let path = state
            .sidecar_path
            .clone()
            .ok_or_else(|| "native capture has no sidecar path".to_string())?;
        Ok::<(PathBuf, NativeCapture), String>((path, build_native_capture(state)?))
    })?;
    persist_capture_atomically(&path, &capture)
}

fn persist_capture_atomically(path: &Path, capture: &NativeCapture) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create native capture sidecar directory {}: {error}", parent.display()))?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|error| format!("failed to create native capture sidecar: {error}"))?;
    serde_json::to_writer(file.as_file_mut(), capture).map_err(|error| format!("failed to serialize native capture sidecar: {error}"))?;
    file.as_file_mut()
        .flush()
        .map_err(|error| format!("failed to flush native capture sidecar: {error}"))?;
    file.as_file()
        .sync_all()
        .map_err(|error| format!("failed to sync native capture sidecar: {error}"))?;
    file.persist(path).map_err(|error| {
        format!(
            "failed to atomically persist native capture sidecar {}: {}",
            path.display(),
            error.error
        )
    })?;
    Ok(())
}

fn build_native_capture(state: &NativeCaptureState) -> Result<NativeCapture, String> {
    if !state.active_operations.is_empty() {
        return Err("native capture contains active operation scopes".to_string());
    }
    if let Some(error) = &state.terminal_error {
        return Err(error.clone());
    }
    let scenario_end_time_unix_nano = state.next_time_unix_nano;
    let run_end_time_unix_nano = scenario_end_time_unix_nano
        .checked_add(state.config.clock_step_unix_nano)
        .ok_or_else(|| "native capture logical clock overflow".to_string())?;
    let has_error = state
        .operations
        .iter()
        .any(|operation| operation.status == Some(NativeStatus::Error));
    let operations = state
        .operations
        .iter()
        .map(|operation| {
            Ok(NativeOperationSpan {
                order: operation.order,
                span_id: operation.span_id.clone(),
                parent_span_id: operation.parent_span_id.clone(),
                component_id: operation.component_id.clone(),
                operation_name: operation.operation_name.clone(),
                start_time_unix_nano: operation.start_time_unix_nano,
                end_time_unix_nano: operation
                    .end_time_unix_nano
                    .ok_or_else(|| "native capture contains an unclosed operation".to_string())?,
                status: operation
                    .status
                    .ok_or_else(|| "native capture contains an operation without status".to_string())?,
                inputs: operation.inputs.clone(),
                observations: operation.observations.clone(),
                completion: operation.completion.clone(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let root_status = if has_error { NativeStatus::Error } else { NativeStatus::Ok };
    Ok(NativeCapture {
        trace_id: state.config.trace_id.clone(),
        scenario_name: state.config.scenario_name.clone(),
        run: NativeSpanBoundary {
            span_id: state.config.run_span_id.clone(),
            parent_span_id: None,
            start_time_unix_nano: state.run_start_time_unix_nano,
            end_time_unix_nano: run_end_time_unix_nano,
            status: root_status,
        },
        scenario: NativeSpanBoundary {
            span_id: state.config.scenario_span_id.clone(),
            parent_span_id: Some(state.config.run_span_id.clone()),
            start_time_unix_nano: state.scenario_start_time_unix_nano,
            end_time_unix_nano: scenario_end_time_unix_nano,
            status: root_status,
        },
        operations,
    })
}

fn validate_native_capture_config(config: &NativeCaptureConfig) -> Result<(), String> {
    validate_hex_id("trace ID", &config.trace_id, 32)?;
    validate_hex_id("run span ID", &config.run_span_id, 16)?;
    validate_hex_id("scenario span ID", &config.scenario_span_id, 16)?;
    if config.start_time_unix_nano < 0 {
        return Err("native capture start timestamp must be non-negative".to_string());
    }
    if config.clock_step_unix_nano <= 0 {
        return Err("native capture logical clock step must be positive".to_string());
    }
    let mut span_ids = HashSet::new();
    span_ids.insert(config.run_span_id.as_str());
    if !span_ids.insert(config.scenario_span_id.as_str()) {
        return Err("native capture span IDs must be unique".to_string());
    }
    for (index, span_id) in config.operation_span_ids.iter().enumerate() {
        validate_hex_id(&format!("operation span ID at index {index}"), span_id, 16)?;
        if !span_ids.insert(span_id.as_str()) {
            return Err(format!("native capture operation span ID at index {index} is duplicated"));
        }
    }
    Ok(())
}

fn validate_hex_id(label: &str, value: &str, length: usize) -> Result<(), String> {
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) || value.bytes().all(|byte| byte == b'0') {
        return Err(format!("{label} must be a non-zero {length}-character hexadecimal string"));
    }
    Ok(())
}

fn ensure_active_operation(state: &NativeCaptureState, operation_index: usize) -> Result<(), String> {
    if state.active_operations.last().copied() == Some(operation_index) {
        Ok(())
    } else if state
        .operations
        .get(operation_index)
        .and_then(|operation| operation.status)
        .is_some()
    {
        Err(format!(
            "native operation '{}' was completed twice",
            state.operations[operation_index].operation_name
        ))
    } else {
        Err(format!(
            "native operation '{}' attempted completion while a nested scope is active",
            state.operations[operation_index].operation_name
        ))
    }
}

fn with_native_state_mut<T>(f: impl FnOnce(&mut NativeCaptureState) -> Result<T, String>) -> Result<T, String> {
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot
            .as_mut()
            .ok_or_else(|| "native capture session ended before operation scope".to_string())?;
        f(state)
    })
}

fn record_native_observation(name: &str, value: &Value) -> Result<(), String> {
    NATIVE_CAPTURE.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(());
        };
        let Some(operation_index) = state.active_operations.last().copied() else {
            return Ok(());
        };
        if name == "$result" || name == "$fault" {
            return Ok(());
        }
        let observation = NativeObservation {
            order: state.order()?,
            time_unix_nano: state.tick()?,
            name: name.to_string(),
            value: value.clone(),
        };
        state.operations[operation_index].observations.push(observation);
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Thread-local legacy trace buffer + mock table.
// ---------------------------------------------------------------------------

thread_local! {
    static BUFFER: RefCell<Vec<TraceEvent>> = const { RefCell::new(Vec::new()) };
    static MOCKS: RefCell<HashMap<String, HashMap<String, String>>> =
        RefCell::new(HashMap::new());
    static NATIVE_CAPTURE: RefCell<Option<NativeCaptureState>> = const { RefCell::new(None) };
}

/// Push an `Event { name, value }` onto the thread-local trace buffer. The
/// `&str`-taking shim is preserved so existing macro expansions and call
/// sites that pass `format!("{}", x)` keep compiling unchanged.
pub fn emit_event(name: &str, value: &str) {
    emit_event_v(name, Value::String(value.to_string()));
}

/// Push a structured `Event { name, value }`.
///
/// # Panics
///
/// Panics if an active native capture's deterministic logical clock overflows.
pub fn emit_event_v(name: &str, value: Value) {
    record_native_observation(name, &value).unwrap_or_else(|error| panic!("failed to record native observation: {error}"));
    let event = TraceEvent::Event {
        name: name.to_string(),
        value,
    };
    record_event(&event);
    BUFFER.with(|b| {
        b.borrow_mut().push(event);
    });
}

/// Record one macro-generated operation input in native capture and in the
/// temporary flat trace consumed by unmigrated extraction/harness paths.
///
/// # Panics
///
/// Panics when the active operation scope rejects a duplicate or out-of-order
/// input.
pub fn emit_input_event_v(scope: &mut OperationScope, name: &str, input_name: &str, native_value: Value, compatibility_value: Value) {
    scope
        .record_input(input_name, native_value)
        .unwrap_or_else(|error| panic!("failed to record native operation input: {error}"));
    emit_compatibility_event_v(name, compatibility_value);
}

/// Complete one macro-generated operation result and record the temporary flat
/// `$result` event consumed by unmigrated extraction/harness paths.
///
/// # Panics
///
/// Panics when the active operation scope is completed twice or out of order.
pub fn emit_result_event_v(scope: &mut OperationScope, native_value: Value, compatibility_value: Value) {
    scope
        .complete_result(native_value)
        .unwrap_or_else(|error| panic!("failed to complete native operation result: {error}"));
    emit_compatibility_event_v("$result", compatibility_value);
}

fn emit_compatibility_event_v(name: &str, value: Value) {
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

/// Like [`emit_event_v`] but writes ONLY to the record file (when
/// `SPECGATE_RECORD` is set) — not to the in-process [`BUFFER`]. Used by
/// `#[spec_setup]` to echo construction inputs so `specgate extract --cases`
/// can reconstruct the case's `setup:` map without polluting the harness trace.
/// No-op when `SPECGATE_RECORD` is unset, matching the natural no-op of
/// `record_event` under the same condition.
pub fn record_event_only(name: &str, value: Value) {
    record_event(&TraceEvent::Event {
        name: name.to_string(),
        value,
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
pub trait SpecEventStruct: SpecEvent + ToSpecValue + ToNativeValue {}

// ---------------------------------------------------------------------------
// ToSpecValue — convert any annotated value to a structured `Value`.
// ---------------------------------------------------------------------------

pub trait ToSpecValue {
    fn to_spec_value(&self) -> Value;
}

/// Convert an annotated value to its native CTSC semantic representation.
///
/// This remains separate from [`ToSpecValue`] because the temporary flat trace
/// used by unmigrated consumers flattens optional inputs while CTSC 0.2
/// preserves their `Some`/`None` discriminant. Implementations recurse through
/// the collection shapes supported by [`ToSpecValue`].
pub trait ToNativeValue {
    fn to_native_value(&self) -> Value;
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

macro_rules! to_native_value_int {
    ($($t:ty),*) => {
        $(impl ToNativeValue for $t {
            fn to_native_value(&self) -> Value { Value::Integer(i64::from(*self)) }
        })*
    };
}
to_native_value_int!(i8, i16, i32, u8, u16, u32);

impl ToNativeValue for isize {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToNativeValue for u64 {
    #[allow(clippy::cast_possible_wrap)]
    fn to_native_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToNativeValue for usize {
    #[allow(clippy::cast_possible_wrap)]
    fn to_native_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToNativeValue for i64 {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl ToNativeValue for f32 {
    fn to_native_value(&self) -> Value {
        Value::Float(f64::from(*self))
    }
}

impl ToNativeValue for f64 {
    fn to_native_value(&self) -> Value {
        Value::Float(*self)
    }
}

impl ToNativeValue for bool {
    fn to_native_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl ToNativeValue for char {
    fn to_native_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToNativeValue for str {
    fn to_native_value(&self) -> Value {
        Value::String(self.to_string())
    }
}

impl ToNativeValue for String {
    fn to_native_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl<T: ToNativeValue> ToNativeValue for Vec<T> {
    fn to_native_value(&self) -> Value {
        Value::List(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue> ToNativeValue for [T] {
    fn to_native_value(&self) -> Value {
        Value::List(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue, const N: usize> ToNativeValue for [T; N] {
    fn to_native_value(&self) -> Value {
        Value::List(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue> ToNativeValue for BTreeMap<String, T> {
    fn to_native_value(&self) -> Value {
        Value::Map(self.iter().map(|(key, value)| (key.clone(), value.to_native_value())).collect())
    }
}

impl<T: ToNativeValue, S: std::hash::BuildHasher> ToNativeValue for HashMap<String, T, S> {
    fn to_native_value(&self) -> Value {
        Value::Map(self.iter().map(|(key, value)| (key.clone(), value.to_native_value())).collect())
    }
}

impl<T: ToNativeValue + Ord> ToNativeValue for BTreeSet<T> {
    fn to_native_value(&self) -> Value {
        Value::Set(self.iter().map(ToNativeValue::to_native_value).collect())
    }
}

impl<T: ToNativeValue + Eq + std::hash::Hash, S: std::hash::BuildHasher> ToNativeValue for HashSet<T, S> {
    fn to_native_value(&self) -> Value {
        let mut values: Vec<Value> = self.iter().map(ToNativeValue::to_native_value).collect();
        values.sort();
        Value::Set(values.into_iter().collect())
    }
}

impl<T: ToNativeValue> ToNativeValue for Option<T> {
    fn to_native_value(&self) -> Value {
        let (variant, payload) = match self {
            Some(value) => ("Some", value.to_native_value()),
            None => ("None", Value::Map(BTreeMap::new())),
        };
        Value::Map(BTreeMap::from([(variant.to_string(), payload)]))
    }
}

impl ToNativeValue for Value {
    fn to_native_value(&self) -> Value {
        self.clone()
    }
}

impl<T: ToNativeValue + ?Sized> ToNativeValue for &T {
    fn to_native_value(&self) -> Value {
        (**self).to_native_value()
    }
}

impl<T: ToNativeValue + ?Sized> ToNativeValue for Box<T> {
    fn to_native_value(&self) -> Value {
        (**self).to_native_value()
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
    fn emit_result(&self, scope: &mut OperationScope);
}

impl<T: SpecEventStruct + ?Sized> ReturnEmitStruct for &&&ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self, scope: &mut OperationScope) {
        self.0.emit_fields(None);
        emit_result_event_v(scope, self.0.to_native_value(), self.0.to_spec_value());
    }
}

// Level 2 — enums / collections / any `ToSpecValue`: structured $result only.
pub trait ReturnEmitToSpec {
    fn emit_result(&self, scope: &mut OperationScope);
}

impl<T: ToSpecValue + ToNativeValue + ?Sized> ReturnEmitToSpec for &&ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self, scope: &mut OperationScope) {
        emit_result_event_v(scope, self.0.to_native_value(), self.0.to_spec_value());
    }
}

// Level 3 — any `Display` value: Display-string $result.
pub trait ReturnEmitDisplay {
    fn emit_result(&self, scope: &mut OperationScope);
}

impl<T: std::fmt::Display + ?Sized> ReturnEmitDisplay for &ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self, scope: &mut OperationScope) {
        let value = Value::String(format!("{}", self.0));
        emit_result_event_v(scope, value.clone(), value);
    }
}

// Level 4 (lowest priority, universal fallback) — any return type: emits nothing.
pub trait ReturnEmitNone {
    fn emit_result(&self, scope: &mut OperationScope);
}

impl<T: ?Sized> ReturnEmitNone for ReturnEmit<'_, T> {
    #[inline]
    fn emit_result(&self, scope: &mut OperationScope) {
        scope
            .complete_unit()
            .unwrap_or_else(|error| panic!("failed to complete native operation: {error}"));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn native_config(operation_span_ids: &[&str]) -> NativeCaptureConfig {
        NativeCaptureConfig {
            scenario_name: "scenario".to_string(),
            trace_id: "11111111111111111111111111111111".to_string(),
            run_span_id: "1111111111111101".to_string(),
            scenario_span_id: "1111111111111102".to_string(),
            operation_span_ids: operation_span_ids.iter().map(|id| (*id).to_string()).collect(),
            start_time_unix_nano: 1_000,
            clock_step_unix_nano: 10,
        }
    }

    #[test]
    fn inactive_operation_scope_is_a_no_op() {
        let mut scope = begin_native_operation("fixture", "noop").unwrap();
        scope.record_input("value", Value::Integer(2)).unwrap();
        scope.complete_result(Value::Integer(4)).unwrap();
        assert_eq!(finish_native_capture().unwrap_err(), "no native capture session is active");
    }

    #[test]
    fn native_capture_records_nesting_inputs_observations_and_results() {
        reset();
        start_native_capture(native_config(&["1111111111111103", "1111111111111104"])).unwrap();
        let mut outer = begin_native_operation("fixture.native", "outer").unwrap();
        outer.record_input("value", Value::Integer(2)).unwrap();
        emit_event_v("before_inner", Value::Bool(true));
        let mut inner = begin_native_operation("fixture.native", "inner").unwrap();
        inner.record_input("value", Value::Integer(3)).unwrap();
        inner.complete_result(Value::Integer(6)).unwrap();
        outer.complete_result(Value::Integer(7)).unwrap();

        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations.len(), 2);
        assert_eq!(capture.operations[0].parent_span_id, capture.scenario.span_id);
        assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
        assert_eq!(capture.operations[0].inputs["value"], Value::Integer(2));
        assert_eq!(capture.operations[0].observations[0].name, "before_inner");
        assert!(matches!(
            capture.operations[1].completion,
            Some(NativeCompletion::Result {
                value: Value::Integer(6),
                ..
            })
        ));
        assert_eq!(capture.run.status, NativeStatus::Ok);
        assert!(capture.operations[0].start_time_unix_nano < capture.operations[1].start_time_unix_nano);
        assert!(capture.operations[1].end_time_unix_nano < capture.operations[0].end_time_unix_nano);
        reset();
    }

    #[test]
    fn native_capture_rejects_invalid_configuration_and_extra_ids() {
        let mut malformed = native_config(&[]);
        malformed.trace_id = "bad".to_string();
        assert!(start_native_capture(malformed).unwrap_err().contains("trace ID"));

        let mut bad_step = native_config(&[]);
        bad_step.clock_step_unix_nano = 0;
        assert!(start_native_capture(bad_step).unwrap_err().contains("clock step"));

        start_native_capture(native_config(&["1111111111111103"])).unwrap();
        assert!(
            finish_native_capture()
                .unwrap_err()
                .contains("supplied 1 operation span IDs but consumed 0")
        );
    }

    #[test]
    fn native_capture_extends_explicit_ids_without_a_fixed_operation_cap() {
        start_native_capture(native_config(&["1111111111111103"])).unwrap();
        let mut scope = begin_native_operation("fixture.native", "outer").unwrap();
        let mut inner = begin_native_operation("fixture.native", "inner").unwrap();
        assert!(finish_native_capture().unwrap_err().contains("nested/unclosed"));
        inner.complete_unit().unwrap();
        scope.complete_unit().unwrap();
        assert!(scope.complete_unit().unwrap_err().contains("completed twice"));
        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations[0].span_id, "1111111111111103");
        assert_ne!(capture.operations[1].span_id, capture.operations[0].span_id);
    }

    #[test]
    fn native_capture_sidecar_types_have_stable_serde() {
        start_native_capture(native_config(&[])).unwrap();
        let mut scope = begin_native_operation("fixture.native", "serde").unwrap();
        scope.record_input("value", Value::Integer(2)).unwrap();
        scope.complete_result(Value::Integer(4)).unwrap();
        let capture = finish_native_capture().unwrap();
        let json = serde_json::to_string(&capture).unwrap();
        assert_eq!(serde_json::from_str::<NativeCapture>(&json).unwrap(), capture);
        assert!(json.contains(r#""status":"ok""#));
        assert!(json.contains(r#""kind":"result""#));
    }

    #[test]
    fn native_capture_keeps_multiple_top_level_invocations_in_one_scenario() {
        start_native_capture(native_config(&[])).unwrap();
        let mut first = begin_native_operation("fixture.native", "first").unwrap();
        first.complete_result(Value::Integer(1)).unwrap();
        let mut second = begin_native_operation("fixture.native", "second").unwrap();
        second.complete_result(Value::Integer(2)).unwrap();

        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations.len(), 2);
        assert_eq!(capture.operations[0].parent_span_id, capture.scenario.span_id);
        assert_eq!(capture.operations[1].parent_span_id, capture.scenario.span_id);
        assert_ne!(capture.operations[0].span_id, capture.operations[1].span_id);
    }

    #[test]
    fn native_capture_marks_unwind_as_unexpected_target_fault() {
        start_native_capture(native_config(&["1111111111111103"])).unwrap();
        let panic = std::panic::catch_unwind(|| {
            let _scope = begin_native_operation("fixture.native", "explode").unwrap();
            panic!("boom");
        });
        assert!(panic.is_err());

        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.run.status, NativeStatus::Error);
        assert_eq!(capture.operations[0].status, NativeStatus::Error);
        assert!(matches!(
            &capture.operations[0].completion,
            Some(NativeCompletion::Fault {
                fault_type,
                observer,
                ..
            }) if fault_type == "specgate.unexpected_target_fault" && observer == "target"
        ));
    }

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
