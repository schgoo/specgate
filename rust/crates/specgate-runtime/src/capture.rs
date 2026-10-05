//! Native synchronous capture sessions, setup folding, and persistence.

use super::*;

thread_local! {
    pub(crate) static SESSION: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub(crate) fn is_active() -> bool {
    SESSION.with(|slot| slot.borrow().is_some())
}

fn requested() -> bool {
    is_active() || env_var(ENV_KEY).is_some_and(|value| !value.is_empty())
}

#[derive(Debug)]
struct TerminalFailure {
    kind: CaptureErrorKind,
    diagnostic: String,
}
impl TerminalFailure {
    fn new(kind: CaptureErrorKind, diagnostic: impl Into<String>) -> Self {
        Self {
            kind,
            diagnostic: diagnostic.into(),
        }
    }
    fn as_error(&self) -> CaptureError {
        CaptureError::message(self.kind, self.diagnostic.clone())
    }
    fn into_error(self) -> CaptureError {
        CaptureError::message(self.kind, self.diagnostic)
    }
}

pub(crate) fn mark_error(error: &CaptureError) {
    SESSION.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut()
            && let Some(state) = slot.as_mut()
        {
            state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic()));
        }
    });
}

/// Parent-process environment key carrying native capture activation JSON.
/// Changing this name breaks CLI-to-runtime capture activation compatibility.
const ENV_KEY: &str = "SPECGATE_NATIVE_CAPTURE";

mod model;
pub use model::{
    Capture, Completion, Config, ConfigBuilder, ConfigDeps, EnvConfig, Observation, OperationSpan, SpanBoundary, SpanBuilder, SpanDeps,
    SpanId, Status, TraceId,
};

#[derive(Debug)]
struct PendingOperation {
    order: u64,
    span_id: SpanId,
    parent_span_id: SpanId,
    component_id: ComponentId,
    operation_name: OperationName,
    start_ns: i64,
    end_ns: Option<i64>,
    status: Option<Status>,
    inputs: BTreeMap<String, Value>,
    /// Operation parameters a registered setup constructs. The registry folds
    /// them away, so the black-box input set carries the setup's construction
    /// inputs instead of the parameter the setup filled.
    filled_parameters: BTreeSet<String>,
    observations: Vec<Observation>,
    completion: Option<Completion>,
}

#[derive(Debug)]
struct State {
    config: Config,
    sidecar_path: Option<PathBuf>,
    file_system: FileSystem,
    run_start_ns: i64,
    scenario_start: i64,
    next_time_ns: i64,
    next_op: usize,
    next_order: u64,
    active_ops: Vec<usize>,
    operations: Vec<PendingOperation>,
    persisted_ops: usize,
    terminal_error: Option<TerminalFailure>,
}

impl State {
    fn tick(&mut self) -> Result<i64, CaptureError> {
        let current = self.next_time_ns;
        self.next_time_ns = current
            .checked_add(self.config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        Ok(current)
    }

    fn order(&mut self) -> Result<u64, CaptureError> {
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
///
/// # Panics
/// Dropping an active scope before completing it panics unless the thread is
/// already unwinding, in which case the scope records the target fault.
#[derive(Debug)]
pub struct OperationScope {
    operation_index: Option<usize>,
    closed: bool,
}

impl OperationScope {
    /// Construct a no-op scope for calls made outside an active capture.
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
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion, or when `project` panics.
    pub fn input_lazy(&mut self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.input(name, project())
    }

    /// Record one already-projected semantic input.
    ///
    /// # Errors
    /// Returns an error when capture state access fails.
    ///
    /// # Panics
    /// Panics when the caller records input after completion, records one name twice, or records while a nested operation is active.
    pub fn input(&mut self, name: impl AsRef<str>, value: Value) -> Result<(), CaptureError> {
        let name = name.as_ref();
        let Some(operation_index) = self.operation_index else {
            return Ok(());
        };
        with_state(|state| {
            ensure_active(state, operation_index);
            let operation = &mut state.operations[operation_index];
            assert!(
                operation.status.is_none(),
                "native operation input cannot be recorded after completion"
            );
            if operation.filled_parameters.contains(name) {
                return Ok(());
            }
            assert!(
                operation.inputs.insert(name.to_string(), value).is_none(),
                "native operation input '{name}' cannot be recorded twice"
            );
            Ok(())
        })?;
        Ok(())
    }

    /// Complete this operation with a typed semantic result.
    ///
    /// # Errors
    ///
    /// Returns an error for logical-clock exhaustion or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion.
    pub fn result(self, value: Value) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Result(value))
    }

    /// Complete with a result projected only for an active capture.
    ///
    /// # Errors
    /// Returns an error for logical-clock exhaustion or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion, or when `project` panics.
    pub fn result_lazy(self, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.result(project())
    }

    /// Complete this operation successfully without a completion event.
    ///
    /// # Errors
    ///
    /// Returns an error for logical-clock exhaustion or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion.
    pub fn unit(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Unit)
    }

    /// Complete this operation through its declared empty channel.
    ///
    /// # Errors
    ///
    /// Returns an error for logical-clock exhaustion or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion.
    pub fn empty(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Empty)
    }

    /// Complete with a declared error projected only for an active capture.
    ///
    /// # Errors
    /// Returns an error for an empty error name, logical-clock exhaustion, or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion, or when `project` panics.
    pub fn error_lazy(self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.error(name, project())
    }

    /// Complete through a declared error with an already-projected payload.
    ///
    /// # Errors
    /// Returns an error for an empty name, logical-clock exhaustion, or top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when this scope is completed after completion or outside LIFO order.
    pub fn error(self, name: impl AsRef<str>, value: Value) -> Result<(), CaptureError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err(CaptureError::from("native declared error name must not be empty".to_string()));
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
    /// Returns an error for an empty error name, logical-clock exhaustion, or
    /// top-level sidecar persistence failure.
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion.
    pub fn error_unit(self, name: impl AsRef<str>) -> Result<(), CaptureError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err(CaptureError::from("native declared error name must not be empty".to_string()));
        }
        self.complete(NativeTerminal::Error {
            name: name.to_string(),
            value: None,
        })
    }

    /// Complete this operation with the stable unexpected-target fault.
    ///
    /// # Errors
    ///
    /// Returns an error for double completion, non-LIFO completion, logical
    /// clock overflow, or top-level sidecar persistence failure.
    #[doc(hidden)]
    ///
    /// # Panics
    /// Panics when the operation is completed out of LIFO order or after completion.
    pub fn unwind(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Fault)
    }

    fn complete(mut self, terminal: NativeTerminal) -> Result<(), CaptureError> {
        let Some(operation_index) = self.operation_index else {
            return Ok(());
        };
        let top_level = with_state(|state| {
            ensure_active(state, operation_index);
            let (completion, status) = match terminal {
                NativeTerminal::Unit => (None, Status::Ok),
                NativeTerminal::Result(value) => (
                    Some(Completion::Result {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                        value,
                    }),
                    Status::Ok,
                ),
                NativeTerminal::Empty => (
                    Some(Completion::Empty {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                    }),
                    Status::Ok,
                ),
                NativeTerminal::Error { name, value } => (
                    Some(Completion::Error {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                        name,
                        value,
                    }),
                    Status::Error,
                ),
                NativeTerminal::Fault => (
                    Some(Completion::Fault {
                        order: state.order()?,
                        time_unix_nano: state.tick()?,
                        fault_type: UNEXPECTED_FAULT.to_string(),
                        message: "operation unwound before returning".to_string(),
                        observer: TARGET_OBSERVER.to_string(),
                    }),
                    Status::Error,
                ),
            };
            let end_ns = state.tick()?;
            let operation = &mut state.operations[operation_index];
            operation.observations.shrink_to_fit();
            operation.completion = completion;
            operation.end_ns = Some(end_ns);
            operation.status = Some(status);
            state.active_ops.pop();
            Ok(state.active_ops.is_empty())
        })?;
        self.closed = true;
        if top_level {
            persist_active()?;
        }
        Ok(())
    }
}

enum NativeTerminal {
    Unit,
    Result(Value),
    Empty,
    Error { name: String, value: Option<Value> },
    Fault,
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let Some(operation_index) = self.operation_index else {
            return;
        };
        SESSION.with(|slot| {
            let Ok(mut slot) = slot.try_borrow_mut() else {
                return;
            };
            let Some(state) = slot.as_mut() else {
                return;
            };
            cleanup_unfinished(state, operation_index, std::thread::panicking());
        });
    }
}

fn cleanup_unfinished(state: &mut State, operation_index: usize, panicking: bool) {
    let Some(operation) = state.operations.get(operation_index) else {
        return;
    };
    if operation.status.is_some() {
        return;
    }
    let operation_name = operation.operation_name.to_string();
    if let Some(position) = state.active_ops.iter().rposition(|active| *active == operation_index) {
        state.active_ops.remove(position);
    }
    assert!(panicking, "native operation '{operation_name}' scope closed without completion");
    let completion = state.order().and_then(|order| {
        let time_unix_nano = state.tick()?;
        Ok(Completion::Fault {
            order,
            time_unix_nano,
            fault_type: UNEXPECTED_FAULT.to_string(),
            message: "operation unwound before returning".to_string(),
            observer: TARGET_OBSERVER.to_string(),
        })
    });
    let end_ns = state.tick();
    match (completion, end_ns) {
        (Ok(completion), Ok(end_ns)) => {
            let operation = &mut state.operations[operation_index];
            operation.completion = Some(completion);
            operation.end_ns = Some(end_ns);
            operation.status = Some(Status::Error);
        }
        (Err(error), _) | (_, Err(error)) => state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic())),
    }
}

/// Start one deterministic thread-local native capture session.
///
/// # Errors
///
/// Rejects malformed or duplicate identifiers, non-positive clock settings, and logical-clock overflow.
///
/// # Panics
/// Panics when the caller attempts to replace an already-active thread-local session.
pub fn start(config: Config) -> Result<(), CaptureError> {
    start_with(config, None, FileSystem::real())
}

fn start_with(config: Config, sidecar_path: Option<PathBuf>, file_system: FileSystem) -> Result<(), CaptureError> {
    validate_config(&config)?;
    if sidecar_path.as_ref().is_some_and(|path| path.as_os_str().is_empty()) {
        return Err("native capture sidecar path must not be empty".to_string().into());
    }
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(slot.is_none(), "a native capture session is already active");
        let scenario_start = config
            .start_time_unix_nano
            .checked_add(config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let next_time_ns = scenario_start
            .checked_add(config.clock_step_unix_nano)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let operation_capacity = config.operation_span_ids.len();
        *slot = Some(State {
            run_start_ns: config.start_time_unix_nano,
            scenario_start,
            next_time_ns,
            config,
            sidecar_path,
            file_system,
            next_op: 0,
            next_order: 0,
            active_ops: Vec::with_capacity(operation_capacity),
            operations: Vec::with_capacity(operation_capacity),
            persisted_ops: 0,
            terminal_error: None,
        });
        Ok(())
    })
}

/// Begin a native operation scope, or return a cheap inactive guard.
///
/// When no capture is active, this returns without projecting payloads or
/// allocating capture state.
///
/// # Errors
///
/// Returns an error when environment activation fails, setup inputs cannot be
/// folded, the deterministic operation ID list is exhausted, or the logical
/// clock cannot advance.
///
/// # Examples
///
/// ```
/// use specgate_runtime::{ComponentId, OperationName, capture};
///
/// let mut operation = capture::begin_operation(
///     ComponentId::from("example.math"),
///     OperationName::from("add"),
/// )?;
/// operation.unit()?;
/// # Ok::<(), specgate_runtime::CaptureError>(())
/// ```
pub fn begin_operation(component_id: ComponentId, operation_name: OperationName) -> Result<OperationScope, CaptureError> {
    begin_active(component_id, operation_name)
}

fn begin_active(component_id: ComponentId, operation_name: OperationName) -> Result<OperationScope, CaptureError> {
    activate_env()?;
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(OperationScope::inactive());
        };
        if let Some(error) = &state.terminal_error {
            return Err(error.as_error());
        }

        let (inputs, setup_params) = folded_inputs(component_id.as_str(), operation_name.as_str())?;
        let span_id = next_span(state)?;
        let parent_span_id = state.active_ops.last().map_or_else(
            || state.config.scenario_span_id.clone(),
            |index| state.operations[*index].span_id.clone(),
        );
        let start_ns = state.tick()?;
        let order = state.order()?;
        let operation_index = state.operations.len();
        state.operations.push(PendingOperation {
            order,
            span_id,
            parent_span_id,
            component_id,
            operation_name,
            start_ns,
            end_ns: None,
            status: None,
            inputs,
            filled_parameters: setup_params,
            observations: Vec::new(),
            completion: None,
        });
        state.next_op += 1;
        state.active_ops.push(operation_index);
        Ok(OperationScope {
            operation_index: Some(operation_index),
            closed: false,
        })
    })
}

mod setup;
use setup::folded_inputs;
pub use setup::{DeferredSetup, SetupProvenance, defer_setup, record_setup};
#[cfg(test)]
use setup::{inputs_match, recorded_inputs};

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
pub fn reject_async(component_id: impl AsRef<str>, operation_name: impl AsRef<str>) -> Result<(), CaptureError> {
    let component_id = component_id.as_ref();
    let operation_name = operation_name.as_ref();
    if requested() {
        Err(format!(
            "native capture of async operation '{component_id}::{operation_name}' is unsupported until capture context is task-safe"
        )
        .into())
    } else {
        Ok(())
    }
}

/// Finish and take the active native capture.
///
/// # Errors
///
/// Returns prior recorded scope errors, unused deterministic operation IDs,
/// and logical clock overflow.
///
/// # Panics
/// Panics when no native capture session is active or operation scopes remain open.
pub fn finish() -> Result<Capture, CaptureError> {
    finish_active()
}

fn finish_active() -> Result<Capture, CaptureError> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.take().expect("cannot finish native capture without an active session");
        if !state.active_ops.is_empty() {
            let names = state
                .active_ops
                .iter()
                .map(|index| state.operations[*index].operation_name.as_str())
                .collect::<Vec<_>>()
                .join(" -> ");
            *slot = Some(state);
            panic!("native capture has nested/unclosed operation scopes: {names}");
        }
        // The session is gone for good from here on, so its recorded setup
        // construction inputs must not reach the next one. Restored sessions
        // above keep theirs.
        setup::clear();
        if let Some(error) = state.terminal_error {
            return Err(error.into_error());
        }
        if state.next_op < state.config.operation_span_ids.len() {
            return Err(format!(
                "native capture supplied {} operation span IDs but consumed {}",
                state.config.operation_span_ids.len(),
                state.next_op
            )
            .into());
        }
        let capture = build_capture(&state)?;
        if state.persisted_ops != state.operations.len()
            && let Some(path) = state.sidecar_path.as_deref()
        {
            let snapshot = CaptureSnapshot::new(&state)?;
            if let Err(error) = persist_atomic(&state.file_system, path, &snapshot) {
                return Err(CaptureError::message(
                    CaptureErrorKind::Persistence,
                    format!("{} {error}", generated::FAILURE_MARKER),
                ));
            }
        }
        Ok(capture)
    })
}

fn persist_active() -> Result<(), CaptureError> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut().expect("native capture session ended before operation persistence");
        let Some(path) = state.sidecar_path.as_deref() else {
            return Ok(());
        };
        if state.persisted_ops == state.operations.len() {
            return Ok(());
        }
        let snapshot = CaptureSnapshot::new(state)?;
        if let Err(error) = persist_atomic(&state.file_system, path, &snapshot) {
            let error = CaptureError::message(CaptureErrorKind::Persistence, format!("{} {error}", generated::FAILURE_MARKER));
            state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic()));
            return Err(error);
        }
        state.persisted_ops = state.operations.len();
        Ok(())
    })
}

#[derive(Default)]
struct Environment {
    backend: EnvironmentBackend,
}
#[derive(Default)]
enum EnvironmentBackend {
    #[default]
    Real,
    #[cfg(feature = "test-util")]
    Fake(BTreeMap<std::ffi::OsString, std::ffi::OsString>),
}
impl Environment {
    fn var_os(&self, name: impl AsRef<std::ffi::OsStr>) -> Option<std::ffi::OsString> {
        let name = name.as_ref();
        match &self.backend {
            EnvironmentBackend::Real => std::env::var_os(name),
            #[cfg(feature = "test-util")]
            EnvironmentBackend::Fake(values) => values.get(name).cloned(),
        }
    }
}
thread_local! {
    static CAPTURE_ENVIRONMENT: RefCell<Environment> = RefCell::new(Environment::default());
}
fn env_var(name: impl AsRef<std::ffi::OsStr>) -> Option<std::ffi::OsString> {
    CAPTURE_ENVIRONMENT.with(|environment| environment.borrow().var_os(name))
}
#[cfg(feature = "test-util")]
pub mod test_util {
    //! Thread-local deterministic environment controls for integration tests.
    //! Values affect lazy native-capture activation on the calling thread only.
    pub use super::io::persistence_stage::PersistenceStage;

    use super::{BTreeMap, CAPTURE_ENVIRONMENT, Config, Environment, EnvironmentBackend, FakeFs, FileSystem, PathBuf, start_with};
    pub use crate::generated::Output;
    use std::ffi::{OsStr, OsString};
    use std::sync::{Arc, Mutex};

    /// Controller for one injected native-capture filesystem.
    #[derive(Debug, Clone)]
    pub struct PersistenceProbe {
        state: Arc<Mutex<FakeFs>>,
    }
    impl PersistenceProbe {
        /// Return the ordered persistence stages attempted so far.
        ///
        /// # Panics
        /// Panics if a test poisoned the fake filesystem mutex.
        #[must_use]
        pub fn calls(&self) -> Vec<PersistenceStage> {
            self.state.lock().expect("fake filesystem mutex poisoned").calls.clone()
        }
        /// Return every successfully replaced sidecar snapshot.
        ///
        /// # Panics
        /// Panics if a test poisoned the fake filesystem mutex.
        #[must_use]
        pub fn snapshots(&self) -> Vec<Vec<u8>> {
            self.state.lock().expect("fake filesystem mutex poisoned").snapshots.clone()
        }
    }
    /// Start capture with an injected filesystem and optional failing stage.
    ///
    /// # Errors
    /// Returns the same typed configuration errors as ordinary capture activation, and rejects an empty sidecar path.
    ///
    /// # Panics
    /// Panics when a native capture session is already active on this thread.
    pub fn start_with_fs(
        config: Config,
        sidecar: impl Into<PathBuf>,
        fail_stage: Option<PersistenceStage>,
    ) -> Result<PersistenceProbe, crate::CaptureError> {
        let (file_system, state) = FileSystem::fake(FakeFs {
            fail_stage,
            ..FakeFs::default()
        });
        start_with(config, Some(sidecar.into()), file_system)?;
        Ok(PersistenceProbe { state })
    }

    /// Capture generated parent-protocol output without writing process stderr.
    #[must_use]
    pub fn capture_output() -> Output {
        crate::generated::capture_output()
    }
    /// Restore real parent-protocol output for this thread.
    pub fn reset_output() {
        crate::generated::reset_output();
    }
    /// Replace the environment visible to native-capture activation on this thread.
    pub fn set_environment(values: impl IntoIterator<Item = (OsString, OsString)>) {
        CAPTURE_ENVIRONMENT.with(|environment| {
            *environment.borrow_mut() = Environment {
                backend: EnvironmentBackend::Fake(values.into_iter().collect::<BTreeMap<_, _>>()),
            };
        });
    }
    /// Restore access to the process environment on this thread.
    pub fn reset_environment() {
        CAPTURE_ENVIRONMENT.with(|environment| *environment.borrow_mut() = Environment::default());
    }
    /// Convenience key conversion for callers assembling deterministic values.
    #[must_use]
    pub fn key(value: impl AsRef<OsStr>) -> OsString {
        value.as_ref().to_os_string()
    }
}

fn activate_env() -> Result<(), CaptureError> {
    if SESSION.with(|slot| slot.borrow().is_some()) {
        return Ok(());
    }
    let Some(encoded) = env_var(ENV_KEY) else {
        return Ok(());
    };
    if encoded.is_empty() {
        return Ok(());
    }
    let environment: EnvConfig = serde_json::from_str(&encoded.to_string_lossy())
        .map_err(|error| format!("invalid SPECGATE_NATIVE_CAPTURE configuration: {error}"))?;
    start_with(environment.capture, Some(environment.sidecar_path), FileSystem::real())
}

fn next_span(state: &State) -> Result<SpanId, CaptureError> {
    if let Some(span_id) = state.config.operation_span_ids.get(state.next_op) {
        return Ok(span_id.clone());
    }

    let mut candidate = u64::try_from(state.next_op)
        .map_err(|_error| "native operation span ID sequence overflow".to_string())?
        .checked_add(1)
        .ok_or_else(|| "native operation span ID sequence overflow".to_string())?;
    let mut span_id = String::with_capacity(SPAN_HEX_LEN);
    loop {
        span_id.clear();
        write!(&mut span_id, "{candidate:0SPAN_HEX_LEN$x}").expect("writing a span ID to a string cannot fail");
        let is_reserved = span_id == state.config.run_span_id.as_str()
            || span_id == state.config.scenario_span_id.as_str()
            || state.operations.iter().any(|operation| operation.span_id.as_str() == span_id);
        if !is_reserved {
            return SpanId::try_from(span_id);
        }
        candidate = candidate
            .checked_add(1)
            .ok_or_else(|| "native operation span ID sequence overflow".to_string())?;
    }
}

mod io;
#[cfg(any(test, feature = "test-util"))]
use io::FakeFs;
use io::{FileSystem, persist_atomic};
mod snapshot;
use snapshot::{CaptureSnapshot, build as build_capture, validate_config, validate_hex_id};

fn ensure_active(state: &State, operation_index: usize) {
    let operation = state
        .operations
        .get(operation_index)
        .expect("native operation index must remain valid");
    assert!(
        operation.status.is_none(),
        "native operation '{}' cannot be completed twice",
        operation.operation_name
    );
    assert_eq!(
        state.active_ops.last().copied(),
        Some(operation_index),
        "native operation '{}' cannot complete while a nested scope is active",
        operation.operation_name
    );
}

fn with_state<T>(f: impl FnOnce(&mut State) -> Result<T, CaptureError>) -> Result<T, CaptureError> {
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let state = slot.as_mut().expect("native capture session ended before operation scope");
        f(state)
    })
}

#[cfg(test)]
fn discard() {
    SESSION.with(|slot| {
        slot.borrow_mut().take();
    });
}

pub(crate) fn record_observation(name: String, value: Value) -> Result<(), CaptureError> {
    // Completion events own these protocol-reserved names, so ordinary
    // observations with the same names are excluded from the captured stream.
    const RESULT_OBSERVATION: &str = "$result";
    const FAULT_OBSERVATION: &str = "$fault";
    SESSION.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return Ok(());
        };
        let Some(operation_index) = state.active_ops.last().copied() else {
            return Ok(());
        };
        if name == RESULT_OBSERVATION || name == FAULT_OBSERVATION {
            return Ok(());
        }
        let observation = Observation {
            order: state.order()?,
            time_unix_nano: state.tick()?,
            name,
            value,
        };
        state.operations[operation_index].observations.push(observation);
        Ok(())
    })
}

#[cfg(test)]
mod tests;
