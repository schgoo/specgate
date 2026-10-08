//! Native capture sessions, async context propagation, setup folding, and persistence.

use super::*;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::task::{Context, Poll};

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

fn requested() -> bool {
    is_active() || env_var(ENV_KEY).is_some_and(|value| !value.is_empty())
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

/// Parent-process environment key carrying native capture activation JSON.
/// Changing this name breaks CLI-to-runtime capture activation compatibility.
const ENV_KEY: &str = "SPECGATE_NATIVE_CAPTURE";

mod model;
pub use model::{
    Capture, Completion, Config, ConfigBuilder, ConfigDeps, EnvConfig, Observation, OperationSpan, SpanBoundary, SpanBuilder, SpanDeps,
    SpanId, Status, TraceId,
};

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
}

impl OperationScope {
    /// Construct a scope that performs no recording.
    #[must_use]
    pub const fn inactive() -> Self {
        Self {
            operation_index: None,
            collector: None,
            closed: true,
        }
    }

    fn recording(&self) -> Option<(usize, CollectorHandle)> {
        Some((self.operation_index?, Arc::clone(self.collector.as_ref()?)))
    }

    /// Record one input, projecting its value only when capture is active.
    pub fn input_lazy(&mut self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.input(name, project())
    }

    /// Record one already-projected input when capture is active.
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
    pub fn result(self, value: Value) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Result(value))
    }
    /// Complete with a result projected only when capture is active.
    pub fn result_lazy(self, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.result(project())
    }
    /// Complete successfully without a terminal value event.
    pub fn unit(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Unit)
    }
    /// Complete successfully with an explicit empty terminal event.
    pub fn empty(self) -> Result<(), CaptureError> {
        self.complete(NativeTerminal::Empty)
    }
    /// Complete with a declared error projected only when capture is active.
    pub fn error_lazy(self, name: impl AsRef<str>, project: impl FnOnce() -> Value) -> Result<(), CaptureError> {
        if self.operation_index.is_none() {
            return Ok(());
        }
        self.error(name, project())
    }
    /// Complete with an already-projected declared error.
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
        let panicking = std::thread::panicking();
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
    begin_active(component_id, operation_name)
}

fn begin_active(component_id: ComponentId, operation_name: OperationName) -> Result<OperationScope, CaptureError> {
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
    })
}

/// Capture context carried by an annotated future from construction onward.
#[derive(Debug)]
#[doc(hidden)]
pub struct CaptureContext {
    collector: Option<CollectorHandle>,
    current_operation: Option<usize>,
}

#[doc(hidden)]
#[must_use]
pub fn capture_async_context() -> CaptureContext {
    let collector = current_collector();
    let current_operation = collector
        .as_ref()
        .and_then(|collector| lock_collector(collector).as_ref().and_then(|state| state.current_operation));
    CaptureContext {
        collector,
        current_operation,
    }
}

struct InstalledContext<'a> {
    context: &'a mut CaptureContext,
    previous_collector: Option<CollectorHandle>,
    previous_operation: Option<usize>,
}
impl<'a> InstalledContext<'a> {
    fn install(context: &'a mut CaptureContext) -> Self {
        let previous_collector = SESSION.with(|slot| slot.replace(context.collector.clone()));
        let previous_operation = swap_current_operation(context.collector.as_ref(), context.current_operation);
        Self {
            context,
            previous_collector,
            previous_operation,
        }
    }
}
impl Drop for InstalledContext<'_> {
    fn drop(&mut self) {
        self.context.current_operation = swap_current_operation(self.context.collector.as_ref(), self.previous_operation);
        SESSION.with(|slot| *slot.borrow_mut() = self.previous_collector.take());
    }
}
fn swap_current_operation(collector: Option<&CollectorHandle>, value: Option<usize>) -> Option<usize> {
    let collector = collector?;
    let mut guard = lock_collector(collector);
    let state = guard.as_mut()?;
    std::mem::replace(&mut state.current_operation, value)
}

#[derive(Debug)]
#[doc(hidden)]
pub struct InstrumentedFuture<F> {
    context: CaptureContext,
    future: Pin<Box<F>>,
}
#[doc(hidden)]
pub fn instrument_async_operation<F: Future>(context: CaptureContext, future: F) -> InstrumentedFuture<F> {
    InstrumentedFuture {
        context,
        future: Box::pin(future),
    }
}
impl<F: Future> Future for InstrumentedFuture<F> {
    type Output = F::Output;
    fn poll(self: Pin<&mut Self>, task: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { context, future } = self.get_mut();
        if context.collector.is_none() {
            return future.as_mut().poll(task);
        }
        let installed = InstalledContext::install(context);
        let result = future.as_mut().poll(task);
        drop(installed);
        result
    }
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

    const fn is_real(&self) -> bool {
        matches!(self.backend, EnvironmentBackend::Real)
    }
}
thread_local! {
    static CAPTURE_ENVIRONMENT: RefCell<Environment> = RefCell::new(Environment::default());
    static REAL_ENV_CHECKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn env_var(name: impl AsRef<std::ffi::OsStr>) -> Option<std::ffi::OsString> {
    CAPTURE_ENVIRONMENT.with(|environment| environment.borrow().var_os(name))
}
fn capture_env_var() -> Option<std::ffi::OsString> {
    CAPTURE_ENVIRONMENT.with(|environment| {
        let environment = environment.borrow();
        if environment.is_real() {
            if REAL_ENV_CHECKED.with(std::cell::Cell::get) {
                return None;
            }
            REAL_ENV_CHECKED.with(|checked| checked.set(true));
        }
        environment.var_os(ENV_KEY)
    })
}
#[cfg(feature = "test-util")]
pub mod test_util {
    //! Thread-local deterministic environment controls for integration tests.
    //! Values affect lazy native-capture activation on the calling thread only.
    pub use super::io::persistence_stage::PersistenceStage;

    use super::{
        BTreeMap, CAPTURE_ENVIRONMENT, Config, Environment, EnvironmentBackend, FakeFs, FileSystem, PathBuf, REAL_ENV_CHECKED, start_with,
    };
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
        REAL_ENV_CHECKED.with(|checked| checked.set(false));
    }
    /// Restore access to the process environment on this thread.
    pub fn reset_environment() {
        CAPTURE_ENVIRONMENT.with(|environment| *environment.borrow_mut() = Environment::default());
        REAL_ENV_CHECKED.with(|checked| checked.set(false));
    }
    /// Convenience key conversion for callers assembling deterministic values.
    #[must_use]
    pub fn key(value: impl AsRef<OsStr>) -> OsString {
        value.as_ref().to_os_string()
    }
}

fn activate_env() -> Result<(), CaptureError> {
    if current_collector().is_some() {
        return Ok(());
    }
    let Some(encoded) = capture_env_var() else {
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
