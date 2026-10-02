//! `SpecGate` runtime — the support library the annotation macros expand into.
//!
//! Provides native structured operation capture, semantic value projection,
//! and the link-time operation/type registry used by CTSC discovery.
//! Isolated test processes can activate native capture through
//! `SPECGATE_NATIVE_CAPTURE`; the first operation starts the session lazily and
//! each completed top-level operation atomically refreshes a stable JSON
//! sidecar containing the full scenario.
//!
//! Captured inputs are the registry's black-box surface, not the raw call:
//! `#[spec_setup]` producers record their construction inputs, and the
//! operation they build adopts those inputs in place of the parameters the
//! setup fills. Attribution is by setup declaration, so running one declaration
//! twice in a capture is accepted only when both runs record value-identical
//! inputs; differing repeats are rejected rather than misattributed.
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
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

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
///
/// [`sidecar_path`](Self::sidecar_path) holds a provisional snapshot: it is
/// rewritten after each operation's inputs are recorded and again at every
/// operation close, and is valid as of the last completed write. Operations
/// still open at a write are projected into it with an `incomplete_capture`
/// fault.
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

#[derive(Debug, Clone)]
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
    /// Operation parameters a registered setup constructs. The registry folds
    /// them away, so the black-box input set carries the setup's construction
    /// inputs instead of the parameter the setup filled.
    setup_filled_parameters: BTreeSet<String>,
    observations: Vec<NativeObservation>,
    completion: Option<NativeCompletion>,
}

/// Link-time identity of one `#[spec_setup]` declaration.
///
/// Stacking several setup annotations on one producer, or registering several
/// producers for one operation, yields distinct keys, so every declaration
/// keeps its own recorded construction inputs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PendingSetupKey {
    component_id: String,
    operation_name: String,
    module_path: String,
    fn_name: String,
    fills: String,
}

/// Mutable state backing the active native capture session.
///
/// Cloning is used only to project a provisional sidecar snapshot; the clone's
/// clock and order advances are discarded rather than applied to the session.
#[derive(Debug, Clone)]
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

/// Shared handle to the collector backing one capture run.
///
/// The recording is owned by the collector, not by the ambient slot: an
/// [`OperationScope`] clones this handle at construction and never consults the
/// ambient slot again. The inner `Option` is the session's liveness —
/// [`finish_native_capture`] takes the recording out of the collector, so a
/// scope that outlives its session still reports "session ended" rather than
/// mutating a finalized recording.
///
/// `Mutex` rather than `RefCell` is what makes the handle `Send`, which is the
/// entire point of the type: an async wrapper can clone it into a future.
type CollectorHandle = Arc<Mutex<Option<NativeCaptureState>>>;

/// Lock one collector, recovering deterministically from poisoning.
///
/// Poisoning is a failure mode `RefCell` did not have, and refusing the lock is
/// the wrong answer here. A capture whose critical section panicked is already
/// destined for an `incomplete_capture` fault on the outstanding operation (see
/// [`close_outstanding_native_operations`]) or for a `terminal_error` recorded
/// by [`OperationScope::drop`]. Returning `Err` forever after would replace
/// that informative artifact with an uninformative "lock poisoned", so the
/// guard is recovered and the recording is allowed to describe its own failure.
fn lock_collector(collector: &CollectorHandle) -> MutexGuard<'_, Option<NativeCaptureState>> {
    collector.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Clone the collector handle installed in the ambient slot, if any.
///
/// This is the only read of the ambient slot's contents. The `RefCell` borrow
/// ends before the returned handle is locked, so no collector lock is ever held
/// across an ambient-slot borrow.
fn current_collector() -> Option<CollectorHandle> {
    NATIVE_CAPTURE.with(|slot| slot.borrow().as_ref().map(Arc::clone))
}

/// Run `f` against one collector's live recording.
///
/// The sole lock acquisition point for mutation. Callers must not perform I/O
/// or acquire another collector lock inside `f`: unlike a `RefCell` double
/// borrow, a reentrant `Mutex` acquisition deadlocks silently.
fn with_collector_mut<T>(collector: &CollectorHandle, f: impl FnOnce(&mut NativeCaptureState) -> Result<T, String>) -> Result<T, String> {
    let mut guard = lock_collector(collector);
    let state = guard
        .as_mut()
        .ok_or_else(|| "native capture session ended before operation scope".to_string())?;
    f(state)
}

/// RAII guard for one annotation-generated synchronous operation invocation.
///
/// A scope owns a shared handle to the collector recording it, captured when
/// the scope is constructed, rather than an index into ambient storage. The
/// inactive representation is allocation-free and is returned whenever no
/// native capture session is active.
#[derive(Debug)]
pub struct OperationScope {
    operation_index: Option<usize>,
    collector: Option<CollectorHandle>,
    closed: bool,
}

impl OperationScope {
    #[must_use]
    pub const fn inactive() -> Self {
        Self {
            operation_index: None,
            collector: None,
            closed: true,
        }
    }

    /// The recording this scope owns a handle to, with its operation index.
    ///
    /// `None` for an inactive scope, which has no session behind it. The handle
    /// is cloned out so a caller can keep using it while mutating the scope
    /// itself (for example setting `closed` before persisting).
    fn recording(&self) -> Option<(usize, CollectorHandle)> {
        match (self.operation_index, self.collector.as_ref()) {
            (Some(operation_index), Some(collector)) => Some((operation_index, Arc::clone(collector))),
            _ => None,
        }
    }

    /// Record one semantic input without adding a native observation.
    ///
    /// # Errors
    ///
    /// Returns an error for an out-of-order scope, a duplicate input, or a
    /// scope that has already completed.
    pub fn record_input(&mut self, name: &str, value: Value) -> Result<(), String> {
        let Some((operation_index, collector)) = self.recording() else {
            return Ok(());
        };
        with_collector_mut(&collector, |state| {
            ensure_active_operation(state, operation_index)?;
            let operation = &mut state.operations[operation_index];
            if operation.status.is_some() {
                return Err(format!("native operation '{}' is already complete", operation.operation_name));
            }
            if operation.setup_filled_parameters.contains(name) {
                return Ok(());
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

    /// Records a provisional snapshot of the capture session.
    ///
    /// `#[spec_operation]` emits this call after recording the operation's
    /// declared inputs and before running its body. Callers must uphold that
    /// ordering: the snapshot's inputs are compared against the inputs the
    /// registry declares for the operation, so a snapshot taken before
    /// [`record_input`](Self::record_input) has run carries an empty input
    /// surface and fails linked validation.
    ///
    /// Does nothing on an inactive scope or when the session has no sidecar.
    ///
    /// # Errors
    ///
    /// Returns an error when the provisional snapshot cannot be persisted.
    pub fn inputs_recorded(&mut self) -> Result<(), String> {
        let Some((_operation_index, collector)) = self.recording() else {
            return Ok(());
        };
        // The lock closes before the write: persistence is I/O and must never
        // run inside a critical section.
        let has_sidecar = with_collector_mut(&collector, |state| Ok(state.sidecar_path.is_some()))?;
        if has_sidecar {
            persist_collector_capture(&collector)?;
        }
        Ok(())
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
        let Some((operation_index, collector)) = self.recording() else {
            return Ok(());
        };
        let should_persist = with_collector_mut(&collector, |state| {
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
            Ok(state.sidecar_path.is_some())
        })?;
        self.closed = true;
        if should_persist {
            // Outside the critical section: see `with_collector_mut`.
            persist_collector_capture(&collector)?;
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
        let Some((operation_index, collector)) = self.recording() else {
            return;
        };
        // M2: `panicking()` answers "did this operation unwind?" correctly for
        // a synchronous call and for a directly-awaited future, because in both
        // cases the thread dropping the scope is the thread that ran it. It is
        // still unreliable for a future that migrates or is abandoned: dropped
        // on an executor thread panicking for an unrelated reason it reports
        // `true`, and an abandoned future is dropped with no panic at all, so
        // this takes the `terminal_error` branch below. That is the accepted
        // terminal-state decision for now; representing abandonment as
        // something other than a poisoned session is M4 and human-owned.
        let panicking = std::thread::panicking();
        let result = with_collector_mut(&collector, |state| {
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
                Ok(state.sidecar_path.is_some())
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
                // Outside the critical section: see `with_collector_mut`.
                if let Err(error) = persist_collector_capture(&collector) {
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

/// Start one deterministic native capture session for this run.
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
        *slot = Some(Arc::new(Mutex::new(Some(NativeCaptureState {
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
        }))));
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
    let Some(collector) = current_collector() else {
        return Ok(OperationScope::inactive());
    };
    // The handle is cloned once, here, and carried by the scope: from this
    // point the scope works from its own collector, never from the ambient
    // slot. That is what makes the recording `Send`-reachable in M2.
    let operation_index = {
        let mut guard = lock_collector(&collector);
        let Some(state) = guard.as_mut() else {
            return Ok(OperationScope::inactive());
        };
        if let Some(error) = &state.terminal_error {
            return Err(error.clone());
        }

        let (inputs, setup_filled_parameters) = folded_setup_inputs(component_id, operation_name)?;
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
            inputs,
            setup_filled_parameters,
            observations: Vec::new(),
            completion: None,
        });
        state.next_operation_id += 1;
        state.active_operations.push(operation_index);
        operation_index
    };
    Ok(OperationScope {
        operation_index: Some(operation_index),
        collector: Some(collector),
        closed: false,
    })
}

#[derive(Debug)]
#[doc(hidden)]
pub struct DeferredSetupInputs {
    component_id: &'static str,
    operation_name: &'static str,
    module_path: &'static str,
    fn_name: &'static str,
    fills: &'static str,
    inputs: Option<Vec<(String, Value)>>,
}

#[doc(hidden)]
#[must_use]
pub fn defer_setup_inputs<F>(
    component_id: &'static str,
    operation_name: &'static str,
    module_path: &'static str,
    fn_name: &'static str,
    fills: &'static str,
    inputs: F,
) -> DeferredSetupInputs
where
    F: FnOnce() -> Vec<(String, Value)>,
{
    DeferredSetupInputs {
        component_id,
        operation_name,
        module_path,
        fn_name,
        fills,
        inputs: native_capture_is_active_or_requested().then(inputs),
    }
}

impl Drop for DeferredSetupInputs {
    fn drop(&mut self) {
        // M2: setups are still synchronous only — an async `#[spec_setup]` is
        // left uninstrumented and its component is rejected before capture, so
        // the thread dropping this guard is always the thread that ran the
        // setup. Instrumenting async setups is M3 and needs the deferred-input
        // state to move into the collector first. See the matching note in
        // `OperationScope::drop`.
        if std::thread::panicking() {
            return;
        }
        let Some(inputs) = self.inputs.take() else {
            return;
        };
        record_setup_inputs(
            self.component_id,
            self.operation_name,
            self.module_path,
            self.fn_name,
            self.fills,
            || inputs,
        )
        .unwrap_or_else(|error| panic!("failed to record native setup inputs: {error}"));
    }
}

/// Record one `#[spec_setup]` producer's semantic construction inputs.
///
/// Registry discovery folds a setup's parameters into the public input surface
/// of the operation it constructs, so the next invocation of that exact
/// component + operation adopts these values as its own black-box inputs.
/// Values are projected only while a capture session is active or requested,
/// so ordinary runs pay nothing and observe no behavior change.
///
/// Attribution is by declaration, not by constructed instance: nothing links a
/// returned receiver back to the call that produced it. Running one setup
/// declaration twice in a single capture is therefore accepted only when both
/// runs record value-identical inputs; differing inputs are ambiguous and are
/// rejected instead of silently attributing the last construction to every
/// later invocation.
///
/// # Errors
///
/// Returns an error when one setup declares the same input name twice, or when
/// one setup declaration runs twice in a capture with differing inputs.
pub fn record_setup_inputs<F>(
    component_id: &str,
    operation_name: &str,
    module_path: &str,
    fn_name: &str,
    fills: &str,
    inputs: F,
) -> Result<(), String>
where
    F: FnOnce() -> Vec<(String, Value)>,
{
    if !native_capture_is_active_or_requested() {
        return Ok(());
    }
    let mut recorded = BTreeMap::new();
    for (name, value) in inputs() {
        if recorded.insert(name.clone(), value).is_some() {
            return Err(format!(
                "setup '{fn_name}' for '{component_id}::{operation_name}' records input '{name}' twice"
            ));
        }
    }
    let key = PendingSetupKey {
        component_id: component_id.to_string(),
        operation_name: operation_name.to_string(),
        module_path: module_path.to_string(),
        fn_name: fn_name.to_string(),
        fills: fills.to_string(),
    };
    PENDING_SETUP_INPUTS.with(|pending| {
        let mut pending = pending.borrow_mut();
        match pending.get(&key) {
            Some(existing) if setup_inputs_are_identical(existing, &recorded) => Ok(()),
            Some(existing) => Err(format!(
                "setup '{fn_name}' for '{component_id}::{operation_name}' ran twice in one capture with different inputs \
                 ({} then {}); capture attributes construction inputs by declaration and cannot tell which instance a later \
                 '{operation_name}' invocation used. Capture one construction per scenario, or record identical inputs.",
                describe_setup_inputs(existing),
                describe_setup_inputs(&recorded)
            )),
            None => {
                pending.insert(key, recorded);
                Ok(())
            }
        }
    })
}

/// Exact structural identity of two recorded construction input maps.
///
/// Deliberately stricter than [`Value`]'s `PartialEq`, which equates a list
/// with a set and an integer with a float. Repeat construction is accepted only
/// when the two recordings are the same value in the same representation.
fn setup_inputs_are_identical(left: &BTreeMap<String, Value>, right: &BTreeMap<String, Value>) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|((left_name, left_value), (right_name, right_value))| {
                left_name == right_name && values_are_identical(left_value, right_value)
            })
}

fn values_are_identical(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::String(left), Value::String(right)) => left == right,
        (Value::Integer(left), Value::Integer(right)) => left == right,
        (Value::Unsigned(left), Value::Unsigned(right)) => left == right,
        (Value::Float(left), Value::Float(right)) => left.to_bits() == right.to_bits(),
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::List(left), Value::List(right)) => {
            left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| values_are_identical(left, right))
        }
        (Value::Set(left), Value::Set(right)) => {
            left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| values_are_identical(left, right))
        }
        (Value::Map(left), Value::Map(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|((left_key, left_value), (right_key, right_value))| {
                        left_key == right_key && values_are_identical(left_value, right_value)
                    })
        }
        _ => false,
    }
}

/// Render one recorded construction input map for an actionable error message.
fn describe_setup_inputs(inputs: &BTreeMap<String, Value>) -> String {
    if inputs.is_empty() {
        return "no inputs".to_string();
    }
    inputs
        .iter()
        .map(|(name, value)| format!("{name}={value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// True when a native capture session is active or the environment requests one.
fn native_capture_is_active_or_requested() -> bool {
    NATIVE_CAPTURE.with(|slot| slot.borrow().is_some())
        || std::env::var_os("SPECGATE_NATIVE_CAPTURE").is_some_and(|value| !value.is_empty())
}

/// Construction inputs recorded for one exact component + operation key.
///
/// Attribution never crosses a component or operation boundary: only setups
/// registered for this exact operation contribute. Discovery rejects a
/// component whose folded surface carries one input name twice, so a duplicate
/// here is a real metadata fault rather than something to silently merge.
fn recorded_setup_inputs(component_id: &str, operation_name: &str) -> Result<BTreeMap<String, Value>, String> {
    PENDING_SETUP_INPUTS.with(|pending| {
        let pending = pending.borrow();
        let mut merged: BTreeMap<String, Value> = BTreeMap::new();
        for (key, inputs) in pending
            .iter()
            .filter(|(key, _inputs)| key.component_id == component_id && key.operation_name == operation_name)
        {
            for (name, value) in inputs {
                if merged.insert(name.clone(), value.clone()).is_some() {
                    return Err(format!(
                        "setups for '{component_id}::{operation_name}' record input '{name}' twice; setup '{}' collides with an earlier producer",
                        key.fn_name
                    ));
                }
            }
        }
        Ok(merged)
    })
}

#[derive(Debug)]
enum SetupContribution {
    Receiver,
    Parameter(String),
}

#[derive(Debug)]
struct ExpectedSetupProvenance {
    key: PendingSetupKey,
    contribution: SetupContribution,
}

fn folded_setup_inputs(component_id: &str, operation_name: &str) -> Result<(BTreeMap<String, Value>, BTreeSet<String>), String> {
    let Some(operation) = SPECGATE_OPS
        .iter()
        .find(|candidate| !candidate.is_setup && candidate.component == component_id && candidate.name == operation_name)
    else {
        return Ok((recorded_setup_inputs(component_id, operation_name)?, BTreeSet::new()));
    };

    let mut filled = BTreeSet::new();
    let mut required_setups = Vec::new();
    let mut receiver_claimed = false;
    for setup in SPECGATE_OPS
        .iter()
        .filter(|candidate| candidate.is_setup && candidate.component == component_id && candidate.name == operation_name)
    {
        let contribution = if setup.fills.is_empty() {
            let candidates = operation
                .params
                .iter()
                .filter(|(name, declared)| {
                    !filled.contains(*name) && normalize_declared_type(declared) == normalize_declared_type(setup.return_type)
                })
                .map(|(name, _declared)| (*name).to_string())
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [only] => Some(SetupContribution::Parameter(only.clone())),
                [] if !receiver_claimed => {
                    receiver_claimed = true;
                    Some(SetupContribution::Receiver)
                }
                _ => None,
            }
        } else if operation.params.iter().any(|(name, _declared)| *name == setup.fills) {
            Some(SetupContribution::Parameter(setup.fills.to_string()))
        } else {
            None
        };

        if let Some(contribution) = contribution {
            if let SetupContribution::Parameter(name) = &contribution {
                filled.insert(name.clone());
            }
            required_setups.push(ExpectedSetupProvenance {
                key: PendingSetupKey {
                    component_id: setup.component.to_string(),
                    operation_name: setup.name.to_string(),
                    module_path: setup.module_path.to_string(),
                    fn_name: setup.fn_name.to_string(),
                    fills: setup.fills.to_string(),
                },
                contribution,
            });
        }
    }

    PENDING_SETUP_INPUTS.with(|pending| {
        let pending = pending.borrow();
        let mut merged = BTreeMap::new();
        for setup in &required_setups {
            let Some(inputs) = pending.get(&setup.key) else {
                return Err(format!(
                    "native capture for '{component_id}::{operation_name}' requires successful provenance from {}; invoke that #[spec_setup] during this capture and let it return successfully before calling '{operation_name}'",
                    describe_expected_setup_provenance(setup)
                ));
            };
            for (name, value) in inputs {
                if merged.insert(name.clone(), value.clone()).is_some() {
                    return Err(format!(
                        "setups for '{component_id}::{operation_name}' record input '{name}' twice; setup '{}' collides with an earlier producer",
                        setup.key.fn_name
                    ));
                }
            }
        }
        Ok((merged, filled))
    })
}

fn describe_expected_setup_provenance(setup: &ExpectedSetupProvenance) -> String {
    let function = format!("'{}::{}'", setup.key.module_path, setup.key.fn_name);
    match &setup.contribution {
        SetupContribution::Receiver => format!("setup {function} constructing the receiver"),
        SetupContribution::Parameter(name) if setup.key.fills.is_empty() => {
            format!("setup {function} inferring folded parameter '{name}'")
        }
        SetupContribution::Parameter(name) => format!("setup {function} filling parameter '{name}'"),
    }
}

/// Collapse declared-type whitespace the way discovery does before comparing.
fn normalize_declared_type(declared: &str) -> String {
    declared.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Reject async operation capture before a future crosses an `.await`.
///
/// Retained for M3, not currently called: `#[spec_operation]` no longer expands
/// to this check, because a directly-awaited async operation records normally
/// from first poll. The remaining unsupported case is an async
/// `#[spec_setup]`, which M3 instruments once deferred setup inputs move into
/// the per-run collector; until then `specgate capture` rejects such a
/// component up front instead. Removing this published function is also an M3
/// concern, so it stays part of the surface for now.
///
/// # Errors
///
/// Returns an explicit unsupported error when an in-process or
/// environment-requested native capture is active.
pub fn reject_async_native_capture(component_id: &str, operation_name: &str) -> Result<(), String> {
    if native_capture_is_active_or_requested() {
        Err(format!(
            "native capture of async operation '{component_id}::{operation_name}' is unsupported until capture context is task-safe"
        ))
    } else {
        Ok(())
    }
}

/// Close every still-outstanding operation with an `incomplete_capture` fault.
///
/// Trace §7.5 requires the fault on the unfinished operation span itself, so
/// every level is closed innermost first and each one carries its own fault.
/// Closing all of them — not just the innermost — is what lets the recording
/// still encode as well-formed spans with an end time and a status.
///
/// Returns the outstanding chain, outermost first, for the caller's error.
fn close_outstanding_native_operations(state: &mut NativeCaptureState) -> Result<String, String> {
    let chain = state
        .active_operations
        .iter()
        .map(|index| state.operations[*index].operation_name.as_str())
        .collect::<Vec<_>>()
        .join(" -> ");
    while let Some(operation_index) = state.active_operations.last().copied() {
        let operation_name = state.operations[operation_index].operation_name.clone();
        let completion = NativeCompletion::Fault {
            order: state.order()?,
            time_unix_nano: state.tick()?,
            fault_type: "incomplete_capture".to_string(),
            message: format!("native operation '{operation_name}' was still outstanding when the capture ended"),
            observer: "target".to_string(),
        };
        let end_time_unix_nano = state.tick()?;
        let operation = &mut state.operations[operation_index];
        operation.completion = Some(completion);
        operation.end_time_unix_nano = Some(end_time_unix_nano);
        operation.status = Some(NativeStatus::Error);
        state.active_operations.pop();
    }
    Ok(chain)
}

/// Finish and take the active native capture.
///
/// An operation still outstanding at this point is closed with an
/// `incomplete_capture` target fault and the recording is persisted, so the
/// artifact says which operation never finished; the call still fails.
///
/// # Errors
///
/// Rejects absent sessions, nested/unclosed scopes, prior scope errors, unused
/// deterministic operation IDs, and logical clock overflow.
pub fn finish_native_capture() -> Result<NativeCapture, String> {
    let Some(collector) = NATIVE_CAPTURE.with(|slot| slot.borrow_mut().take()) else {
        return Err("no native capture session is active".to_string());
    };
    // Take the recording out of the collector as well, not just out of the
    // ambient slot: a scope still holding a handle must see a finished session
    // rather than keep mutating a finalized recording.
    let Some(mut state) = lock_collector(&collector).take() else {
        return Err("no native capture session is active".to_string());
    };
    // An outstanding operation is a contract violation the recording still
    // has to describe: emit the fault, persist what was observed, and then
    // fail the run anyway.
    let outstanding = if state.active_operations.is_empty() {
        None
    } else {
        Some(close_outstanding_native_operations(&mut state)?)
    };
    // The session is gone for good from here on, so its recorded setup
    // construction inputs must not reach the next one.
    PENDING_SETUP_INPUTS.with(|pending| pending.borrow_mut().clear());
    if let Some(chain) = outstanding {
        let message = format!("native capture has nested/unclosed operation scopes: {chain}");
        if let Some(path) = state.sidecar_path.clone()
            && let Err(error) = build_native_capture(&state).and_then(|capture| persist_capture_atomically(&path, &capture))
        {
            return Err(format!("{message}; the incomplete capture could not be persisted: {error}"));
        }
        return Err(message);
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

/// Rewrites one collector's sidecar from its current session state.
///
/// With an empty operation stack the recording is complete and is persisted
/// from live state. With operations still open the snapshot is projected from a
/// clone on which [`close_outstanding_native_operations`] has been run, so the
/// open operations appear with an `incomplete_capture` fault. The projection
/// must stay on a clone: its clock and order advances are discarded, leaving
/// the live session's sequence unchanged.
///
/// The projection is built under the lock and the lock is released before the
/// write: no I/O ever runs inside a critical section.
fn persist_collector_capture(collector: &CollectorHandle) -> Result<(), String> {
    let (path, capture) = {
        let guard = lock_collector(collector);
        let state = guard
            .as_ref()
            .ok_or_else(|| "native capture session ended before snapshot persistence".to_string())?;
        let path = state
            .sidecar_path
            .clone()
            .ok_or_else(|| "native capture has no sidecar path".to_string())?;
        let capture = if state.active_operations.is_empty() {
            build_native_capture(state)?
        } else {
            let mut provisional = state.clone();
            close_outstanding_native_operations(&mut provisional)?;
            build_native_capture(&provisional)?
        };
        (path, capture)
    };
    persist_capture_atomically(&path, &capture)
}

/// How many times a sidecar replacement is retried before it is reported.
///
/// One scenario rewrites its sidecar at every operation boundary, so the same
/// destination is replaced many times in a few milliseconds. On Windows a
/// replacement transiently fails with "access is denied" whenever another
/// process — a virus scanner, a search indexer — still holds the file it just
/// saw appear. Retrying is not papering over a race in the capture itself: the
/// serialized bytes are already complete and durable, and only the final rename
/// is retried.
const SIDECAR_PERSIST_ATTEMPTS: u32 = 12;

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
    for attempt in 1..=SIDECAR_PERSIST_ATTEMPTS {
        match file.persist(path) {
            Ok(_persisted) => return Ok(()),
            Err(rejected) if attempt < SIDECAR_PERSIST_ATTEMPTS => {
                file = rejected.file;
                std::thread::sleep(std::time::Duration::from_millis(u64::from(attempt) * 10));
            }
            Err(rejected) => {
                return Err(format!(
                    "failed to atomically persist native capture sidecar {} after {SIDECAR_PERSIST_ATTEMPTS} attempts: {}",
                    path.display(),
                    rejected.error
                ));
            }
        }
    }
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

fn record_native_observation(name: &str, value: &Value) -> Result<(), String> {
    let Some(collector) = current_collector() else {
        return Ok(());
    };
    let mut guard = lock_collector(&collector);
    let Some(state) = guard.as_mut() else {
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
}

// ---------------------------------------------------------------------------
// Native observations and semantic value projection.
// ---------------------------------------------------------------------------

thread_local! {
    /// Ambient slot naming the collector for the current run.
    ///
    /// The slot stays thread-local — that is what keeps concurrent `#[test]`
    /// functions isolated from one another. Only its *contents* became shared:
    /// the recording now lives behind a `Send` handle that an `OperationScope`
    /// clones at construction.
    static NATIVE_CAPTURE: RefCell<Option<CollectorHandle>> = const { RefCell::new(None) };
    static PENDING_SETUP_INPUTS: RefCell<BTreeMap<PendingSetupKey, BTreeMap<String, Value>>> =
        const { RefCell::new(BTreeMap::new()) };
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
    fn setup_inputs_are_recorded_only_while_a_session_is_active() {
        assert!(recorded_setup_inputs("fixture.native", "increment").unwrap().is_empty());
        record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(4))]
        })
        .unwrap();
        assert!(
            recorded_setup_inputs("fixture.native", "increment").unwrap().is_empty(),
            "an inactive, unrequested capture records nothing"
        );

        start_native_capture(native_config(&[])).unwrap();
        record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(4))]
        })
        .unwrap();
        let mut scope = begin_native_operation("fixture.native", "increment").unwrap();
        scope.complete_unit().unwrap();

        let capture = finish_native_capture().unwrap();
        assert_eq!(
            capture.operations[0].inputs,
            BTreeMap::from([("initial".to_string(), Value::Integer(4))])
        );
        assert!(
            recorded_setup_inputs("fixture.native", "increment").unwrap().is_empty(),
            "finishing a session clears recorded setup inputs"
        );
    }

    #[test]
    fn setup_inputs_reject_duplicate_names_instead_of_guessing() {
        start_native_capture(native_config(&[])).unwrap();
        record_setup_inputs("fixture.native", "combine", "fixture", "make", "left", || {
            vec![("seed".to_string(), Value::Integer(1))]
        })
        .unwrap();
        record_setup_inputs("fixture.native", "combine", "fixture", "make", "right", || {
            vec![("seed".to_string(), Value::Integer(2))]
        })
        .unwrap();
        assert_eq!(
            recorded_setup_inputs("fixture.native", "combine").unwrap_err(),
            "setups for 'fixture.native::combine' record input 'seed' twice; setup 'make' collides with an earlier producer"
        );
        assert_eq!(
            record_setup_inputs("fixture.native", "combine", "fixture", "make", "left", || {
                vec![("seed".to_string(), Value::Integer(1)), ("seed".to_string(), Value::Integer(2))]
            })
            .unwrap_err(),
            "setup 'make' for 'fixture.native::combine' records input 'seed' twice"
        );
        finish_native_capture().unwrap();
    }

    #[test]
    fn repeating_one_setup_declaration_with_identical_inputs_is_accepted() {
        start_native_capture(native_config(&[])).unwrap();
        for _run in 0..2 {
            record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
                vec![("initial".to_string(), Value::Integer(4))]
            })
            .unwrap();
        }
        assert_eq!(
            recorded_setup_inputs("fixture.native", "increment").unwrap(),
            BTreeMap::from([("initial".to_string(), Value::Integer(4))]),
            "a repeat construction with the same inputs is unambiguous"
        );
        finish_native_capture().unwrap();
    }

    #[test]
    fn repeating_one_setup_declaration_with_different_inputs_is_rejected() {
        start_native_capture(native_config(&[])).unwrap();
        record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(4))]
        })
        .unwrap();
        let error = record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(9))]
        })
        .unwrap_err();
        assert_eq!(
            error,
            "setup 'make' for 'fixture.native::increment' ran twice in one capture with different inputs \
             (initial=Integer(4) then initial=Integer(9)); capture attributes construction inputs by declaration and \
             cannot tell which instance a later 'increment' invocation used. Capture one construction per scenario, or \
             record identical inputs."
        );
        assert_eq!(
            recorded_setup_inputs("fixture.native", "increment").unwrap(),
            BTreeMap::from([("initial".to_string(), Value::Integer(4))]),
            "a rejected repeat leaves the first construction untouched"
        );
        finish_native_capture().unwrap();
    }

    #[test]
    fn repeat_setup_identity_is_exact_rather_than_value_equality() {
        assert!(!setup_inputs_are_identical(
            &BTreeMap::from([("seed".to_string(), Value::Integer(1))]),
            &BTreeMap::from([("seed".to_string(), Value::Float(1.0))])
        ));
        assert!(!setup_inputs_are_identical(
            &BTreeMap::from([("seed".to_string(), Value::List(vec![Value::Integer(1)]))]),
            &BTreeMap::from([("seed".to_string(), Value::Set(BTreeSet::from([Value::Integer(1)])))])
        ));
        assert!(!setup_inputs_are_identical(
            &BTreeMap::from([("seed".to_string(), Value::Integer(1))]),
            &BTreeMap::new()
        ));
        assert!(setup_inputs_are_identical(
            &BTreeMap::from([(
                "seed".to_string(),
                Value::Map(BTreeMap::from([("a".to_string(), Value::Bool(true))]))
            )]),
            &BTreeMap::from([(
                "seed".to_string(),
                Value::Map(BTreeMap::from([("a".to_string(), Value::Bool(true))]))
            )])
        ));
    }

    #[test]
    fn a_finished_session_leaves_no_setup_inputs_for_the_next_one() {
        start_native_capture(native_config(&[])).unwrap();
        record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(4))]
        })
        .unwrap();
        finish_native_capture().unwrap();

        start_native_capture(native_config(&[])).unwrap();
        assert!(
            recorded_setup_inputs("fixture.native", "increment").unwrap().is_empty(),
            "a new session must not inherit the previous session's construction inputs"
        );
        record_setup_inputs("fixture.native", "increment", "fixture", "make", "", || {
            vec![("initial".to_string(), Value::Integer(9))]
        })
        .unwrap();
        assert_eq!(
            recorded_setup_inputs("fixture.native", "increment").unwrap(),
            BTreeMap::from([("initial".to_string(), Value::Integer(9))]),
            "stale state would have made this differing repeat ambiguous"
        );
        finish_native_capture().unwrap();
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

    /// Leave one operation outstanding by skipping its `Drop`, the way a leaked
    /// `OperationScope` does. `ManuallyDrop` is the sanctioned form of the leak.
    fn leak_scope(scope: OperationScope) {
        let _outstanding = std::mem::ManuallyDrop::new(scope);
    }

    /// Read the sidecar a failed `finish_native_capture` left behind.
    fn read_sidecar(path: &Path) -> NativeCapture {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn assert_incomplete_capture_fault(operation: &NativeOperationSpan) {
        let Some(NativeCompletion::Fault {
            fault_type,
            message,
            observer,
            ..
        }) = &operation.completion
        else {
            panic!("expected an incomplete_capture fault, got {:?}", operation.completion);
        };
        assert_eq!(fault_type, "incomplete_capture");
        assert_eq!(observer, "target");
        assert!(
            message.contains(&operation.operation_name),
            "the fault must name its own operation: {message}"
        );
        assert_eq!(operation.status, NativeStatus::Error);
    }

    #[test]
    fn an_outstanding_operation_is_faulted_persisted_and_still_fails_the_run() {
        let scratch = tempfile::tempdir().unwrap();
        let sidecar = scratch.path().join("capture.json");
        start_native_capture_with_sidecar(native_config(&[]), Some(sidecar.clone())).unwrap();
        leak_scope(begin_native_operation("fixture.native", "leaked").unwrap());

        let error = finish_native_capture().unwrap_err();
        assert_eq!(error, "native capture has nested/unclosed operation scopes: leaked");

        let capture = read_sidecar(&sidecar);
        assert_eq!(capture.operations.len(), 1);
        assert_incomplete_capture_fault(&capture.operations[0]);
        assert!(capture.operations[0].end_time_unix_nano > capture.operations[0].start_time_unix_nano);
        assert_eq!(capture.run.status, NativeStatus::Error);
    }

    #[test]
    fn nested_outstanding_operations_are_all_closed_with_their_own_fault() {
        let scratch = tempfile::tempdir().unwrap();
        let sidecar = scratch.path().join("capture.json");
        start_native_capture_with_sidecar(native_config(&[]), Some(sidecar.clone())).unwrap();
        leak_scope(begin_native_operation("fixture.native", "outer").unwrap());
        leak_scope(begin_native_operation("fixture.native", "inner").unwrap());

        let error = finish_native_capture().unwrap_err();
        assert_eq!(
            error, "native capture has nested/unclosed operation scopes: outer -> inner",
            "the error still names the outstanding chain, outermost first"
        );

        let capture = read_sidecar(&sidecar);
        assert_eq!(capture.operations.len(), 2);
        for operation in &capture.operations {
            assert_incomplete_capture_fault(operation);
        }
        assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
        assert!(
            capture.operations[1].end_time_unix_nano < capture.operations[0].end_time_unix_nano,
            "the innermost operation must close first"
        );
    }

    /// A sole operation leaked without completing is still described by the
    /// sidecar, with its inputs and an `incomplete_capture` fault.
    #[test]
    fn a_leaked_sole_operation_leaves_a_faulted_provisional_sidecar() {
        let scratch = tempfile::tempdir().unwrap();
        let sidecar = scratch.path().join("capture.json");
        start_native_capture_with_sidecar(native_config(&[]), Some(sidecar.clone())).unwrap();
        let mut leaked = begin_native_operation("fixture.native", "leaked").unwrap();
        leaked.record_input("value", Value::Integer(2)).unwrap();
        // Mirrors the prologue `#[spec_operation]` generates: snapshot after inputs.
        leaked.inputs_recorded().unwrap();
        leak_scope(leaked);

        let capture = read_sidecar(&sidecar);
        assert_eq!(capture.operations.len(), 1);
        assert_eq!(capture.operations[0].operation_name, "leaked");
        assert_eq!(
            capture.operations[0].inputs["value"],
            Value::Integer(2),
            "the snapshot must carry the declared inputs, or the bundle cannot link"
        );
        assert_incomplete_capture_fault(&capture.operations[0]);
        assert_eq!(capture.run.status, NativeStatus::Error);
    }

    #[test]
    fn a_leaked_outer_operation_keeps_its_completed_inner_operation() {
        let scratch = tempfile::tempdir().unwrap();
        let sidecar = scratch.path().join("capture.json");
        start_native_capture_with_sidecar(native_config(&[]), Some(sidecar.clone())).unwrap();
        let mut outer = begin_native_operation("fixture.native", "outer").unwrap();
        outer.inputs_recorded().unwrap();
        let mut inner = begin_native_operation("fixture.native", "inner").unwrap();
        inner.inputs_recorded().unwrap();
        inner.complete_result(Value::Integer(6)).unwrap();
        leak_scope(outer);

        let capture = read_sidecar(&sidecar);
        assert_eq!(capture.operations.len(), 2);
        assert_incomplete_capture_fault(&capture.operations[0]);
        assert!(matches!(
            capture.operations[1].completion,
            Some(NativeCompletion::Result {
                value: Value::Integer(6),
                ..
            })
        ));
        assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
    }

    /// A recording whose operations all complete is identical to what
    /// [`finish_native_capture`] builds, unaffected by the snapshots taken
    /// along the way.
    #[test]
    fn provisional_snapshots_do_not_perturb_a_completed_recording() {
        let scratch = tempfile::tempdir().unwrap();
        let sidecar = scratch.path().join("capture.json");
        start_native_capture_with_sidecar(native_config(&["1111111111111103"]), Some(sidecar.clone())).unwrap();
        let mut outer = begin_native_operation("fixture.native", "outer").unwrap();
        outer.inputs_recorded().unwrap();
        emit_event("checkpoint", &BTreeMap::from([("count".to_string(), 2_i32)]));
        let mut inner = begin_native_operation("fixture.native", "inner").unwrap();
        inner.inputs_recorded().unwrap();
        inner.complete_result(Value::Integer(6)).unwrap();
        outer.complete_result(Value::Integer(7)).unwrap();

        let persisted = read_sidecar(&sidecar);
        assert_eq!(
            persisted,
            finish_native_capture().unwrap(),
            "the final sidecar must equal the recording the existing path builds"
        );
    }

    /// [`OperationScope::inputs_recorded`] succeeds and does nothing on an
    /// inactive scope, which has no session behind it.
    #[test]
    fn the_snapshot_hook_is_a_no_op_on_an_inactive_scope() {
        let mut inactive = OperationScope::inactive();
        inactive.inputs_recorded().expect("an inactive scope has no session to snapshot");
    }

    #[test]
    fn a_faulted_incomplete_session_is_taken_rather_than_left_behind() {
        start_native_capture(native_config(&[])).unwrap();
        leak_scope(begin_native_operation("fixture.native", "leaked").unwrap());
        assert_eq!(
            finish_native_capture().unwrap_err(),
            "native capture has nested/unclosed operation scopes: leaked"
        );
        assert_eq!(
            finish_native_capture().unwrap_err(),
            "no native capture session is active",
            "a finalized session must not stay in the slot"
        );
        start_native_capture(native_config(&[])).unwrap();
        finish_native_capture().unwrap();
    }

    /// The structural invariant this collector exists to establish: the
    /// recording is reachable through a `Send` handle, and the scope that holds
    /// it is itself `Send`. An `#[spec_operation] async fn` holds its
    /// `OperationScope` across every `.await` in the body, so a non-`Send`
    /// field added to any of these would make the generated future non-`Send`
    /// at every call site rather than failing here.
    const fn assert_send<T: Send>() {}
    const _: () = assert_send::<NativeCaptureState>();
    const _: () = assert_send::<CollectorHandle>();
    const _: () = assert_send::<OperationScope>();

    /// An `OperationScope` works from the handle it captured at construction,
    /// not from the ambient slot. Detaching the slot entirely must leave the
    /// scope able to record and complete its own operation.
    ///
    /// Deliberately a pure data-structure test: no macro expansion, and no
    /// recording across threads (that is the undetermined-parent case, M2+).
    #[test]
    fn a_scope_records_through_its_own_handle_after_the_ambient_slot_is_detached() {
        start_native_capture(native_config(&[])).unwrap();
        let mut scope = begin_native_operation("fixture.native", "detached").unwrap();

        let collector = NATIVE_CAPTURE
            .with(|slot| slot.borrow_mut().take())
            .expect("the session installs a collector handle in the ambient slot");

        scope.record_input("value", Value::Integer(2)).unwrap();
        scope.complete_result(Value::Integer(3)).unwrap();

        NATIVE_CAPTURE.with(|slot| *slot.borrow_mut() = Some(collector));
        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations.len(), 1);
        assert_eq!(capture.operations[0].inputs["value"], Value::Integer(2));
        assert_eq!(capture.operations[0].status, NativeStatus::Ok);
    }

    /// A scope that outlives its session reports that the session ended and
    /// cannot reach the next one.
    ///
    /// This pins the reason the collector holds `Option<NativeCaptureState>`
    /// rather than the state directly. `finish_native_capture` takes the
    /// recording out of the handle as well as out of the ambient slot, so a
    /// stranded scope finds its own emptied collector. Were the handle to own
    /// the state outright, the scope would keep a finalized recording alive and
    /// silently mutate it; were the scope still an index into the ambient slot,
    /// it would land in whichever session is installed now and pop *that*
    /// session's operation stack.
    #[test]
    fn a_scope_outliving_its_session_cannot_reach_the_next_one() {
        start_native_capture(native_config(&[])).unwrap();
        let mut stranded = begin_native_operation("fixture.native", "stranded").unwrap();
        assert_eq!(
            finish_native_capture().unwrap_err(),
            "native capture has nested/unclosed operation scopes: stranded"
        );

        start_native_capture(native_config(&[])).unwrap();
        let mut successor = begin_native_operation("fixture.native", "successor").unwrap();
        assert_eq!(
            stranded.record_input("value", Value::Integer(1)).unwrap_err(),
            "native capture session ended before operation scope"
        );
        leak_scope(stranded);

        successor.complete_result(Value::Integer(4)).unwrap();
        let capture = finish_native_capture().unwrap();
        assert_eq!(capture.operations.len(), 1, "the successor session must be untouched");
        assert_eq!(capture.operations[0].operation_name, "successor");
        assert_eq!(capture.operations[0].status, NativeStatus::Ok);
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
