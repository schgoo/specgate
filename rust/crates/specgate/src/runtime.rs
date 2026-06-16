use std::cell::RefCell;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationKind {
    Stateless,
    StateMachine,
    Sequence,
    ErrorMap,
    Structural,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Annotation {
    SpecOperation {
        operation: String,
        kind: OperationKind,
        symbol: String,
    },
    SpecSetup {
        operation: String,
        name: String,
        symbol: String,
        #[serde(default)]
        params: Vec<String>,
        returns: String,
    },
    SpecCheckpoint {
        operation: String,
        symbol: String,
    },
    SpecCapture {
        operation: String,
        symbol: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        capture_all: bool,
    },
    SpecMock {
        operation: String,
        symbol: String,
        #[serde(rename = "mock_name")]
        mock_name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TraceEvent {
    OperationEnter {
        operation: String,
        symbol: String,
    },
    OperationExit {
        operation: String,
        symbol: String,
    },
    CaptureAfter {
        operation: String,
        field: String,
        value: String,
    },
    CaptureBefore {
        operation: String,
        field: String,
        value: String,
    },
    Checkpoint {
        operation: String,
        symbol: String,
        value: String,
    },
    MockCall {
        operation: String,
        mock_name: String,
        returned: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotField {
    pub field: &'static str,
    pub value: String,
}

impl SnapshotField {
    #[must_use]
    pub fn new(field: &'static str, value: String) -> Self {
        Self { field, value }
    }
}

pub trait CaptureSnapshot {
    fn specgate_snapshot(&self, operation: &str) -> Vec<SnapshotField>;
}

#[derive(Default)]
struct RuntimeState {
    traces: Vec<TraceEvent>,
    checkpoints: Vec<String>,
    mocks: BTreeMap<String, Value>,
}

thread_local! {
    static STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState::default());
}

pub fn reset() {
    STATE.with(|state| {
        *state.borrow_mut() = RuntimeState::default();
    });
}

#[must_use]
pub fn drain_traces() -> Vec<TraceEvent> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let traces = state.traces.clone();
        state.traces.clear();
        traces
    })
}

#[must_use]
pub fn drain_checkpoints() -> Vec<String> {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let checkpoints = state.checkpoints.clone();
        state.checkpoints.clear();
        checkpoints
    })
}

pub fn install_mock<T>(name: &str, value: T)
where
    T: Serialize,
{
    STATE.with(|state| {
        state.borrow_mut().mocks.insert(
            name.to_string(),
            serde_json::to_value(value).expect("mock value should serialize"),
        );
    });
}

pub fn mock_value<T>(name: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    STATE.with(|state| {
        state
            .borrow()
            .mocks
            .get(name)
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
    })
}

pub fn operation_enter(operation: &str, symbol: &str) {
    push_trace(TraceEvent::OperationEnter {
        operation: operation.to_string(),
        symbol: symbol.to_string(),
    });
}

pub fn operation_exit(operation: &str, symbol: &str) {
    push_trace(TraceEvent::OperationExit {
        operation: operation.to_string(),
        symbol: symbol.to_string(),
    });
}

pub fn capture_before<T>(operation: &str, value: &T)
where
    T: CaptureSnapshot,
{
    for field in value.specgate_snapshot(operation) {
        push_trace(TraceEvent::CaptureBefore {
            operation: operation.to_string(),
            field: field.field.to_string(),
            value: field.value,
        });
    }
}

pub fn capture_after<T>(operation: &str, value: &T)
where
    T: CaptureSnapshot,
{
    for field in value.specgate_snapshot(operation) {
        push_trace(TraceEvent::CaptureAfter {
            operation: operation.to_string(),
            field: field.field.to_string(),
            value: field.value,
        });
    }
}

pub fn checkpoint<T>(operation: &str, symbol: &str, value: T) -> T
where
    T: Serialize,
{
    let rendered = stringify_value(&value);
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.checkpoints.push(rendered.clone());
        state.traces.push(TraceEvent::Checkpoint {
            operation: operation.to_string(),
            symbol: symbol.to_string(),
            value: rendered,
        });
    });
    value
}

pub fn mock_call<T>(operation: &str, mock_name: &str, value: &T)
where
    T: Serialize,
{
    push_trace(TraceEvent::MockCall {
        operation: operation.to_string(),
        mock_name: mock_name.to_string(),
        returned: stringify_value(value),
    });
}

#[must_use]
pub fn method_symbol<T>(method: &str) -> String {
    format!("{}::{method}", std::any::type_name::<T>())
}

#[must_use]
pub fn stringify_value<T>(value: &T) -> String
where
    T: Serialize,
{
    match serde_json::to_value(value).expect("trace value should serialize") {
        Value::Null => "null".to_string(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(string) => string,
        other => serde_json::to_string(&other).expect("json value should stringify"),
    }
}

fn push_trace(event: TraceEvent) {
    STATE.with(|state| {
        state.borrow_mut().traces.push(event);
    });
}
