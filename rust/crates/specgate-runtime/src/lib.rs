//! `SpecGate` runtime — the support library the annotation macros expand into.
//!
//! Provides native structured operation capture, semantic value projection,
//! and the link-time operation/type registry used by CTSC discovery.
//! Isolated test processes can activate native capture through
//! `SPECGATE_NATIVE_CAPTURE`; the first operation starts the session lazily and
//! each completed top-level operation atomically refreshes a stable JSON
//! sidecar containing the full scenario.
//!
//! Companion to the `specgate-annotations-macros` proc-macro crate: the macros expand
//! into calls into this runtime, so user code never references it directly.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

pub use linkme;

// ---------------------------------------------------------------------------
// Operation registry — populated at link time via #[distributed_slice].
// Discovery binaries iterate this to find all annotated operations.
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

/// One enum variant: its name plus either named fields or an ordered tuple
/// payload. Unit variants carry neither.
#[derive(Debug, Clone)]
pub struct VariantMeta {
    pub name: &'static str,
    pub fields: &'static [FieldMeta],
    pub tuple: Option<&'static [&'static str]>,
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
            out.push_str(",\"tuple\":");
            if let Some(tuple) = v.tuple {
                out.push('[');
                for (index, field_type) in tuple.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    let _ = write!(out, "\"{}\"", json_escape(field_type));
                }
                out.push(']');
            } else {
                out.push_str("null");
            }
            out.push('}');
        }
        out.push_str("]}");
    }
    out.push_str("]}");
    out
}

// ---------------------------------------------------------------------------
// Value — native semantic payload.
// ---------------------------------------------------------------------------

/// Structured semantic value used by native capture and CTSC encoding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Value {
    String(String),
    Integer(i64),
    Unsigned(u64),
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
    match v {
        Value::Bool(_) => 0,
        Value::Integer(_) => 1,
        Value::Unsigned(_) => 2,
        Value::Float(_) => 3,
        Value::String(_) => 4,
        Value::List(_) => 5,
        Value::Set(_) => 6,
        Value::Map(_) => 7,
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
            (Value::Unsigned(a), Value::Unsigned(b)) => a == b,
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
    Empty {
        order: u64,
        time_unix_nano: i64,
    },
    Error {
        order: u64,
        time_unix_nano: i64,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<Value>,
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
        self.complete(NativeTerminal::Result(value))
    }

    /// Complete this operation successfully without a completion event.
    ///
    /// # Errors
    ///
    /// Returns an error for double completion or non-LIFO completion.
    pub fn complete_unit(&mut self) -> Result<(), String> {
        self.complete(NativeTerminal::Unit)
    }

    /// Complete this operation through its declared empty channel.
    ///
    /// # Errors
    ///
    /// Returns an error for double completion or non-LIFO completion.
    pub fn complete_empty(&mut self) -> Result<(), String> {
        self.complete(NativeTerminal::Empty)
    }

    /// Complete this operation through a declared error channel.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty error name, double completion, or
    /// non-LIFO completion.
    pub fn complete_error(&mut self, name: &str, value: Value) -> Result<(), String> {
        if name.is_empty() {
            return Err("native declared error name must not be empty".to_string());
        }
        self.complete(NativeTerminal::Error {
            name: name.to_string(),
            value: Some(value),
        })
    }

    /// Complete this operation through a valueless declared error channel.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty error name, double completion, or
    /// non-LIFO completion.
    pub fn complete_error_unit(&mut self, name: &str) -> Result<(), String> {
        if name.is_empty() {
            return Err("native declared error name must not be empty".to_string());
        }
        self.complete(NativeTerminal::Error {
            name: name.to_string(),
            value: None,
        })
    }

    fn complete(&mut self, terminal: NativeTerminal) -> Result<(), String> {
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
            let (completion, status) = match terminal {
                NativeTerminal::Unit => (None, NativeStatus::Ok),
                NativeTerminal::Result(value) => (
                    Some(NativeCompletion::Result {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                        value,
                    }),
                    NativeStatus::Ok,
                ),
                NativeTerminal::Empty => (
                    Some(NativeCompletion::Empty {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                    }),
                    NativeStatus::Ok,
                ),
                NativeTerminal::Error { name, value } => (
                    Some(NativeCompletion::Error {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                        name,
                        value,
                    }),
                    NativeStatus::Error,
                ),
            };
            let end_time_unix_nano = state.tick()?;
            let operation = &mut state.operations[operation_index];
            operation.completion = completion;
            operation.end_time_unix_nano = Some(end_time_unix_nano);
            operation.status = Some(status);
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

enum NativeTerminal {
    Unit,
    Result(Value),
    Empty,
    Error { name: String, value: Option<Value> },
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

/// Reject async operation capture before a future crosses an `.await`.
///
/// Native capture state is thread-local and cannot safely follow a future that
/// migrates between executor threads. Async operations remain discoverable but
/// execute without instrumentation when capture is inactive.
///
/// # Errors
///
/// Returns an explicit unsupported error when an in-process or
/// environment-requested native capture is active.
pub fn reject_async_native_capture(component_id: &str, operation_name: &str) -> Result<(), String> {
    let active = NATIVE_CAPTURE.with(|slot| slot.borrow().is_some());
    let requested = std::env::var_os("SPECGATE_NATIVE_CAPTURE").is_some_and(|value| !value.is_empty());
    if active || requested {
        Err(format!(
            "native capture of async operation '{component_id}::{operation_name}' is unsupported until capture context is task-safe"
        ))
    } else {
        Ok(())
    }
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
// Native observations and semantic value projection.
// ---------------------------------------------------------------------------

thread_local! {
    static NATIVE_CAPTURE: RefCell<Option<NativeCaptureState>> = const { RefCell::new(None) };
}

/// Record a typed observation on the currently active operation.
///
/// # Panics
///
/// Panics if the active native capture cannot advance its deterministic clock
/// or event ordering.
pub fn emit_event<T: ToNativeValue + ?Sized>(name: &str, value: &T) {
    let value = value.to_native_value();
    record_native_observation(name, &value).unwrap_or_else(|error| panic!("failed to record native observation: {error}"));
}

/// Marker implemented by `#[derive(SpecEvent)]` for semantic record/union types.
pub trait SpecEvent: ToNativeValue {}

/// Convert a value to its native CTSC semantic representation.
pub trait ToNativeValue {
    fn to_native_value(&self) -> Value;
}

macro_rules! native_signed {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToNativeValue for $ty {
                fn to_native_value(&self) -> Value {
                    Value::Integer(i64::from(*self))
                }
            }
        )*
    };
}

macro_rules! native_unsigned {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ToNativeValue for $ty {
                fn to_native_value(&self) -> Value {
                    Value::Unsigned(u64::from(*self))
                }
            }
        )*
    };
}

native_signed!(i8, i16, i32);
native_signed!(u8, u16, u32);
native_unsigned!(u64);

impl ToNativeValue for i64 {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self)
    }
}

impl ToNativeValue for isize {
    fn to_native_value(&self) -> Value {
        Value::Integer(*self as i64)
    }
}

impl ToNativeValue for usize {
    fn to_native_value(&self) -> Value {
        Value::Unsigned(*self as u64)
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

impl ToNativeValue for () {
    fn to_native_value(&self) -> Value {
        Value::Map(BTreeMap::new())
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
        self.as_slice().to_native_value()
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
        let mut values = self.iter().map(ToNativeValue::to_native_value).collect::<Vec<_>>();
        values.sort();
        Value::Set(values.into_iter().collect())
    }
}

impl<T: ToNativeValue> ToNativeValue for Option<T> {
    fn to_native_value(&self) -> Value {
        match self {
            Some(value) => Value::Map(BTreeMap::from([("Some".to_string(), value.to_native_value())])),
            None => Value::Map(BTreeMap::from([("None".to_string(), Value::Map(BTreeMap::new()))])),
        }
    }
}

impl<T: ToNativeValue, E: ToNativeValue> ToNativeValue for Result<T, E> {
    fn to_native_value(&self) -> Value {
        match self {
            Ok(value) => Value::Map(BTreeMap::from([("Ok".to_string(), value.to_native_value())])),
            Err(error) => Value::Map(BTreeMap::from([("Err".to_string(), error.to_native_value())])),
        }
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

macro_rules! native_tuple {
    ($($name:ident),+ $(,)?) => {
        impl<$($name: ToNativeValue),+> ToNativeValue for ($($name,)+) {
            #[allow(non_snake_case)]
            fn to_native_value(&self) -> Value {
                let ($($name,)+) = self;
                Value::List(vec![$($name.to_native_value()),+])
            }
        }
    };
}

native_tuple!(A, B);
native_tuple!(A, B, C);
native_tuple!(A, B, C, D);

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
    fn inactive_scope_is_a_no_op() {
        let mut scope = begin_native_operation("fixture", "noop").unwrap();
        scope.record_input("value", Value::Integer(2)).unwrap();
        scope.complete_result(Value::Integer(4)).unwrap();
        assert_eq!(finish_native_capture().unwrap_err(), "no native capture session is active");
    }

    #[test]
    fn captures_nesting_observations_structures_and_dynamic_ids() {
        start_native_capture(native_config(&["1111111111111103"])).unwrap();
        let mut outer = begin_native_operation("fixture.native", "outer").unwrap();
        outer.record_input("optional", Some(vec![1_i32, 2_i32]).to_native_value()).unwrap();
        emit_event("checkpoint", &BTreeMap::from([("count".to_string(), 2_i32)]));
        let mut inner = begin_native_operation("fixture.native", "inner").unwrap();
        inner.complete_result(Value::Integer(6)).unwrap();
        outer.complete_result(Value::Integer(7)).unwrap();

        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations.len(), 2);
        assert_eq!(capture.operations[0].parent_span_id, capture.scenario.span_id);
        assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
        assert_ne!(capture.operations[0].span_id, capture.operations[1].span_id);
        assert_eq!(capture.operations[0].observations[0].name, "checkpoint");
        assert!(matches!(&capture.operations[0].inputs["optional"], Value::Map(values) if values.contains_key("Some")));
    }

    #[test]
    fn captures_empty_declared_error_and_unwind_faults() {
        start_native_capture(native_config(&[])).unwrap();
        let mut empty = begin_native_operation("fixture.native", "empty").unwrap();
        empty.complete_empty().unwrap();
        let mut error = begin_native_operation("fixture.native", "error").unwrap();
        error.complete_error("invalid", Value::String("bad".to_string())).unwrap();
        let panic = std::panic::catch_unwind(|| {
            let _fault = begin_native_operation("fixture.native", "fault").unwrap();
            panic!("boom");
        });
        assert!(panic.is_err());

        let capture = finish_native_capture().unwrap();
        assert!(matches!(capture.operations[0].completion, Some(NativeCompletion::Empty { .. })));
        assert!(matches!(capture.operations[1].completion, Some(NativeCompletion::Error { .. })));
        assert!(matches!(capture.operations[2].completion, Some(NativeCompletion::Fault { .. })));
        assert_eq!(capture.run.status, NativeStatus::Error);
    }

    #[test]
    fn capture_sidecar_types_have_stable_serde() {
        start_native_capture(native_config(&[])).unwrap();
        let mut scope = begin_native_operation("fixture.native", "serde").unwrap();
        scope.record_input("large", Value::Unsigned(u64::MAX)).unwrap();
        scope.complete_result(Value::Integer(4)).unwrap();
        let capture = finish_native_capture().unwrap();
        let json = serde_json::to_string(&capture).unwrap();
        assert_eq!(serde_json::from_str::<NativeCapture>(&json).unwrap(), capture);
    }

    #[test]
    fn rejects_invalid_configuration_and_unused_ids() {
        let mut malformed = native_config(&[]);
        malformed.trace_id = "bad".to_string();
        assert!(start_native_capture(malformed).unwrap_err().contains("trace ID"));

        start_native_capture(native_config(&["1111111111111103"])).unwrap();
        assert!(finish_native_capture().unwrap_err().contains("consumed 0"));
    }
}
