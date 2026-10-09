//! Native capture sessions, async context propagation, setup folding, and persistence.

use super::*;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

type CollectorHandle = Arc<Mutex<Option<State>>>;

thread_local! {
    static SESSION: RefCell<Option<CollectorHandle>> = const { RefCell::new(None) };
}

fn lock_collector(collector: &CollectorHandle) -> MutexGuard<'_, Option<State>> {
    collector.lock().unwrap_or_else(PoisonError::into_inner)
}

fn current_collector() -> Option<CollectorHandle> {
    SESSION.with(|slot| slot.borrow().as_ref().map(Arc::clone))
}

pub(crate) fn is_active() -> bool {
    current_collector().is_some()
}

#[derive(Debug, Clone)]
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
    if let Some(collector) = current_collector()
        && let Some(state) = lock_collector(&collector).as_mut()
    {
        state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic()));
    }
}

mod model;
pub use model::{
    Capture, Completion, Config, ConfigBuilder, ConfigDeps, EnvConfig, Observation, OperationSpan, SpanBoundary, SpanBuilder, SpanDeps,
    SpanId, Status, TraceId,
};
mod async_context;
pub use async_context::{CaptureContext, InstrumentedFuture, capture_async_context, instrument_async_operation};
mod environment;
#[cfg(feature = "test-util")]
pub use environment::test_util;
use environment::{activate_env, requested};

#[derive(Debug, Clone)]
struct PendingOperation {
    order: u64,
    span_id: SpanId,
    parent_span_id: SpanId,
    parent_operation: Option<usize>,
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

#[derive(Debug, Clone)]
struct State {
    config: Config,
    sidecar_path: Option<PathBuf>,
    file_system: FileSystem,
    run_start_ns: i64,
    scenario_start: i64,
    next_time_ns: i64,
    next_op: usize,
    next_order: u64,
    current_operation: Option<usize>,
    operations: Vec<PendingOperation>,
    terminal_error: Option<TerminalFailure>,
}

impl State {
    fn tick(&mut self) -> Result<i64, CaptureError> {
        let current = self.next_time_ns;
        self.next_time_ns = current
            .checked_add(self.config.step_ns)
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

/// Guard for one annotation-generated operation invocation.
///
/// The scope owns the synchronized per-run collector, so it may move with an
/// instrumented future. Destruction performs only in-memory cleanup; all
/// persistence happens at explicit input or completion boundaries.
#[derive(Debug)]
pub struct OperationScope {
    operation_index: Option<usize>,
    collector: Option<CollectorHandle>,
    closed: bool,
    /// Whether dropping this scope without a completion is abandonment rather
    /// than a bug. Only [`begin_async_operation`] sets it: a synchronous body
    /// cannot stop part-way without unwinding.
    abandonable: bool,
}

impl OperationScope {
    /// Construct a scope that performs no recording.
    #[must_use]
    pub const fn inactive() -> Self {
        Self {
            operation_index: None,
            collector: None,
            closed: true,
            abandonable: false,
        }
    }

    fn recording(&self) -> Option<(usize, CollectorHandle)> {
        Some((self.operation_index?, Arc::clone(self.collector.as_ref()?)))
    }

    /// Record one input, projecting its value only when capture is active.
    ///
    /// # Errors
    ///
    /// Returns a terminal capture failure when the active collector is no
    /// longer available.
    ///
    /// # Panics
    ///
    /// Panics if the same input is recorded twice or after completion.
    pub fn input_lazy(&mut self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.input(name, project())
    }

    /// Record one already-projected input when capture is active.
    ///
    /// # Errors
    ///
    /// Returns a terminal capture failure when the active collector is no
    /// longer available.
    ///
    /// # Panics
    ///
    /// Panics if the same input is recorded twice or after completion.
    pub fn input(&mut self, name: impl AsRef<str>, value: Value) -> Result<(), CaptureError> {
        let Some((operation_index, collector)) = self.recording() else {
            return Ok(());
        };
        let name = name.as_ref();
        with_collector_mut(&collector, |state| {
            let operation = state
                .operations
                .get_mut(operation_index)
                .expect("native operation index must remain valid");
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
        })
    }

    /// Persist a provisional snapshot after all declared inputs are recorded.
    ///
    /// # Errors
    ///
    /// Returns terminal collector or sidecar-persistence failures.
    pub fn inputs_recorded(&mut self) -> Result<(), CaptureError> {
        let Some((_index, collector)) = self.recording() else {
            return Ok(());
        };
        let has_sidecar = with_collector_mut(&collector, |state| Ok(state.sidecar_path.is_some()))?;
        if has_sidecar {
            persist_collector_capture(&collector)?;
        }
        Ok(())
    }

    /// Complete the operation with an already-projected result.
    ///
    /// # Errors
    ///
    /// Returns logical-clock, collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn result(self, value: Value) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Result(value))
    }
    /// Complete with a result projected only when capture is active.
    ///
    /// # Errors
    ///
    /// Returns logical-clock, collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn result_lazy(self, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.result(project())
    }
    /// Complete successfully without a terminal value event.
    ///
    /// # Errors
    ///
    /// Returns logical-clock, collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn unit(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Unit)
    }
    /// Complete successfully with an explicit empty terminal event.
    ///
    /// # Errors
    ///
    /// Returns logical-clock, collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn empty(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Empty)
    }
    /// Complete with a declared error projected only when capture is active.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty error name or for logical-clock,
    /// collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn error_lazy(self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.error(name, project())
    }
    /// Complete with an already-projected declared error.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty error name or for logical-clock,
    /// collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn error(self, name: impl AsRef<str>, value: Value) -> Result<(), CaptureError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err("native declared error name must not be empty".to_string().into());
        }
        self.complete(NativeTerminal::Error {
            name: name.to_string(),
            value: Some(value),
        })
    }
    /// Complete with a unit-valued declared error.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty error name or for logical-clock,
    /// collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    pub fn error_unit(self, name: impl AsRef<str>) -> Result<(), CaptureError> {
        let name = name.as_ref();
        if name.is_empty() {
            return Err("native declared error name must not be empty".to_string().into());
        }
        self.complete(NativeTerminal::Error {
            name: name.to_string(),
            value: None,
        })
    }
    /// Complete a target unwind as a fault before resuming its panic.
    ///
    /// # Errors
    ///
    /// Returns logical-clock, collector, or sidecar-persistence failures.
    ///
    /// # Panics
    ///
    /// Panics if the operation was already completed.
    #[doc(hidden)]
    pub fn unwind(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Fault)
    }

    fn complete(mut self, terminal: NativeTerminal) -> Result<(), CaptureError> {
        let Some((operation_index, collector)) = self.recording() else {
            return Ok(());
        };
        let should_persist = with_collector_mut(&collector, |state| {
            let operation = state
                .operations
                .get(operation_index)
                .expect("native operation index must remain valid");
            assert!(
                operation.status.is_none(),
                "native operation '{}' cannot be completed twice",
                operation.operation_name
            );
            let (completion, status) = match terminal {
                NativeTerminal::Unit => (None, Status::Ok),
                NativeTerminal::Result(value) => (
                    Some(Completion::Result {
                        order: state.order()?,
                        time_ns: state.tick()?,
                        value,
                    }),
                    Status::Ok,
                ),
                NativeTerminal::Empty => (
                    Some(Completion::Empty {
                        order: state.order()?,
                        time_ns: state.tick()?,
                    }),
                    Status::Ok,
                ),
                NativeTerminal::Error { name, value } => (
                    Some(Completion::Error {
                        order: state.order()?,
                        time_ns: state.tick()?,
                        name,
                        value,
                    }),
                    Status::Error,
                ),
                NativeTerminal::Fault => (
                    Some(Completion::Fault {
                        order: state.order()?,
                        time_ns: state.tick()?,
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
            release_operation(state, operation_index);
            Ok(state.sidecar_path.is_some())
        })?;
        self.closed = true;
        if should_persist {
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
    Fault,
}

impl Drop for OperationScope {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        let Some((operation_index, collector)) = self.recording() else {
            return;
        };
        // `panicking()` answers "did this operation unwind?" correctly for a
        // synchronous call and for an awaited future, because in both cases the
        // thread dropping the scope is the thread that ran the body.
        //
        // It is checked *before* the abandonable flag, and that ordering has a
        // visible consequence: a future abandoned on a thread that is unwinding
        // for an unrelated reason is recorded as `specgate.unexpected_target_fault`
        // rather than abandoned. That is accepted. The other ordering would
        // record a genuinely panicking async body as merely abandoned, silently
        // losing a real failure on a run that otherwise looks clean.
        // Over-reporting on an already-failing run beats under-reporting on a
        // passing one.
        let panicking = std::thread::panicking();
        let abandonable = self.abandonable;
        let mut guard = lock_collector(&collector);
        let Some(state) = guard.as_mut() else {
            return;
        };
        if state
            .operations
            .get(operation_index)
            .and_then(|operation| operation.status)
            .is_some()
        {
            return;
        }
        if panicking {
            let completion = state.order().and_then(|order| {
                let time_ns = state.tick()?;
                Ok(Completion::Fault {
                    order,
                    time_ns,
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
        } else if abandonable {
            // The future was dropped before it resolved. Nothing went wrong and
            // no contract was satisfied, so this is neither a fault nor a
            // completion: it is a terminal state of its own, with UNSET status
            // and no propagation to the parent operation.
            let completion = state.order().and_then(|order| {
                let time_ns = state.tick()?;
                Ok(Completion::Abandoned { order, time_ns })
            });
            let end_ns = state.tick();
            match (completion, end_ns) {
                (Ok(completion), Ok(end_ns)) => {
                    let operation = &mut state.operations[operation_index];
                    operation.completion = Some(completion);
                    operation.end_ns = Some(end_ns);
                    operation.status = Some(Status::Unset);
                }
                (Err(error), _) | (_, Err(error)) => state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic())),
            }
        } else {
            let diagnostic = format!(
                "native operation '{}' scope closed without completion",
                state.operations[operation_index].operation_name
            );
            state.terminal_error = Some(TerminalFailure::new(CaptureErrorKind::Configuration, diagnostic));
        }
        release_operation(state, operation_index);
    }
}

/// Start one deterministic thread-local native capture session.
///
/// # Errors
///
/// Rejects malformed or duplicate identifiers, non-positive clock settings,
/// logical-clock overflow, and attempts to start while a session is active.
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
        if slot.is_some() {
            return Err("a native capture session is already active".to_string().into());
        }
        let scenario_start = config
            .start_ns
            .checked_add(config.step_ns)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let next_time_ns = scenario_start
            .checked_add(config.step_ns)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let operation_capacity = config.operation_ids.len();
        *slot = Some(Arc::new(Mutex::new(Some(State {
            run_start_ns: config.start_ns,
            scenario_start,
            next_time_ns,
            config,
            sidecar_path,
            file_system,
            next_op: 0,
            next_order: 0,
            current_operation: None,
            operations: Vec::with_capacity(operation_capacity),
            terminal_error: None,
        }))));
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
    begin_active(component_id, operation_name, false)
}

/// Begin a native operation scope for an asynchronous body.
///
/// Identical to [`begin_operation`] except that dropping the returned scope
/// without a completion records abandonment instead of poisoning the session.
/// Only an async body can stop part-way without unwinding, so only an async
/// body earns that permission.
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
/// let mut operation = capture::begin_async_operation(
///     ComponentId::from("example.math"),
///     OperationName::from("add"),
/// )?;
/// operation.unit()?;
/// # Ok::<(), specgate_runtime::CaptureError>(())
/// ```
pub fn begin_async_operation(component_id: ComponentId, operation_name: OperationName) -> Result<OperationScope, CaptureError> {
    begin_active(component_id, operation_name, true)
}

fn begin_active(component_id: ComponentId, operation_name: OperationName, abandonable: bool) -> Result<OperationScope, CaptureError> {
    activate_env()?;
    let Some(collector) = current_collector() else {
        return Ok(OperationScope::inactive());
    };
    let operation_index = {
        let mut guard = lock_collector(&collector);
        let Some(state) = guard.as_mut() else {
            return Ok(OperationScope::inactive());
        };
        if let Some(error) = &state.terminal_error {
            return Err(error.as_error());
        }
        let (inputs, setup_params) = folded_inputs(component_id.as_str(), operation_name.as_str())?;
        let span_id = next_span(state)?;
        let parent_operation = state.current_operation;
        let parent_span_id = parent_operation.map_or_else(
            || state.config.scenario_span_id.clone(),
            |index| state.operations[index].span_id.clone(),
        );
        let start_ns = state.tick()?;
        let order = state.order()?;
        let operation_index = state.operations.len();
        state.operations.push(PendingOperation {
            order,
            span_id,
            parent_span_id,
            parent_operation,
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
        state.current_operation = Some(operation_index);
        operation_index
    };
    Ok(OperationScope {
        operation_index: Some(operation_index),
        collector: Some(collector),
        closed: false,
        abandonable,
    })
}

mod setup;
use setup::folded_inputs;
pub use setup::{DeferredSetup, SetupProvenance, defer_setup, record_setup};
#[cfg(test)]
use setup::{inputs_match, recorded_inputs};

/// Finish and take the active native capture.
///
/// # Errors
///
/// Returns prior recorded scope errors, unused deterministic operation IDs,
/// logical clock overflow, and sidecar persistence failures.
///
/// # Panics
/// Panics when no native capture session is active or operation scopes remain open.
pub fn finish() -> Result<Capture, CaptureError> {
    finish_active()
}

fn finish_active() -> Result<Capture, CaptureError> {
    let collector = SESSION
        .with(|slot| slot.borrow_mut().take())
        .expect("cannot finish native capture without an active session");
    let mut state = lock_collector(&collector)
        .take()
        .expect("cannot finish native capture without an active session");
    setup::clear();
    if has_outstanding(&state) {
        let names = outstanding(&state)
            .into_iter()
            .map(|index| state.operations[index].operation_name.as_str())
            .collect::<Vec<_>>()
            .join(" -> ");
        close_outstanding(&mut state)?;
        if state.sidecar_path.is_some() {
            persist_state(&mut state)?;
        }
        return Err(format!("native capture has nested/unclosed operation scopes: {names}").into());
    }
    if let Some(error) = state.terminal_error.take() {
        return Err(error.into_error());
    }
    if state.next_op < state.config.operation_ids.len() {
        return Err(format!(
            "native capture supplied {} operation span IDs but consumed {}",
            state.config.operation_ids.len(),
            state.next_op
        )
        .into());
    }
    let capture = build_capture(&state)?;
    if state.sidecar_path.is_some() {
        persist_state(&mut state)?;
    }
    Ok(capture)
}

fn outstanding(state: &State) -> Vec<usize> {
    state
        .operations
        .iter()
        .enumerate()
        .filter(|(_, operation)| operation.status.is_none())
        .map(|(index, _)| index)
        .collect()
}
fn has_outstanding(state: &State) -> bool {
    state.operations.iter().any(|operation| operation.status.is_none())
}
fn release_operation(state: &mut State, operation_index: usize) {
    if state.current_operation == Some(operation_index) {
        state.current_operation = state.operations[operation_index].parent_operation;
    }
}
fn close_outstanding(state: &mut State) -> Result<(), CaptureError> {
    for operation_index in outstanding(state).into_iter().rev() {
        let operation_name = state.operations[operation_index].operation_name.to_string();
        let completion = Completion::Fault {
            order: state.order()?,
            time_ns: state.tick()?,
            fault_type: "incomplete_capture".to_string(),
            message: format!("native operation '{operation_name}' was still outstanding when the capture ended"),
            observer: TARGET_OBSERVER.to_string(),
        };
        let end_ns = state.tick()?;
        let operation = &mut state.operations[operation_index];
        operation.completion = Some(completion);
        operation.end_ns = Some(end_ns);
        operation.status = Some(Status::Error);
        release_operation(state, operation_index);
    }
    Ok(())
}
fn persist_state(state: &mut State) -> Result<(), CaptureError> {
    let path = state.sidecar_path.as_deref().expect("persistence requires a sidecar path");
    let snapshot = CaptureSnapshot::new(state)?;
    if let Err(error) = persist_atomic(&state.file_system, path, &snapshot) {
        let error = CaptureError::message(CaptureErrorKind::Persistence, format!("{} {error}", generated::FAILURE_MARKER));
        state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic()));
        return Err(error);
    }
    Ok(())
}
fn persist_collector_capture(collector: &CollectorHandle) -> Result<(), CaptureError> {
    let (file_system, path, capture) = {
        let guard = lock_collector(collector);
        let state = guard
            .as_ref()
            .ok_or_else(|| CaptureError::from("native capture session ended before snapshot persistence".to_string()))?;
        let path = state
            .sidecar_path
            .clone()
            .ok_or_else(|| CaptureError::from("native capture has no sidecar path".to_string()))?;
        let mut provisional = state.clone();
        if has_outstanding(&provisional) {
            close_outstanding(&mut provisional)?;
        }
        let capture = build_capture(&provisional)?;
        (state.file_system.clone(), path, capture)
    };
    if let Err(error) = persist_atomic(&file_system, &path, &capture) {
        let error = CaptureError::message(CaptureErrorKind::Persistence, format!("{} {error}", generated::FAILURE_MARKER));
        if let Some(state) = lock_collector(collector).as_mut() {
            state.terminal_error = Some(TerminalFailure::new(error.kind(), error.diagnostic()));
        }
        return Err(error);
    }
    Ok(())
}

fn next_span(state: &State) -> Result<SpanId, CaptureError> {
    if let Some(span_id) = state.config.operation_ids.get(state.next_op) {
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

fn with_collector_mut<T>(collector: &CollectorHandle, f: impl FnOnce(&mut State) -> Result<T, CaptureError>) -> Result<T, CaptureError> {
    let mut guard = lock_collector(collector);
    let state = guard
        .as_mut()
        .ok_or_else(|| CaptureError::from("native capture session ended before operation scope".to_string()))?;
    f(state)
}

pub(crate) fn record_observation(name: String, value: Value) -> Result<(), CaptureError> {
    const RESULT_OBSERVATION: &str = "$result";
    const FAULT_OBSERVATION: &str = "$fault";
    let Some(collector) = current_collector() else {
        return Ok(());
    };
    with_collector_mut(&collector, |state| {
        let Some(operation_index) = state.current_operation else {
            return Ok(());
        };
        if name == RESULT_OBSERVATION || name == FAULT_OBSERVATION {
            return Ok(());
        }
        let observation = Observation {
            order: state.order()?,
            time_ns: state.tick()?,
            name,
            value,
        };
        state.operations[operation_index].observations.push(observation);
        Ok(())
    })
}

#[cfg(test)]
mod tests;
