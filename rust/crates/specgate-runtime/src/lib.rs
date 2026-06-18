//! SpecGate runtime — thread-local trace buffer + mock table + SpecEvent trait.
//!
//! Companion to the `specgate-annotations` proc-macro crate. The macros
//! expand into calls into this runtime; user code never references this
//! crate directly.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;

/// Trace event emitted by annotated code at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TraceEvent {
    Event { name: String, value: String },
    Run { operation: String },
}

impl TraceEvent {
    /// Returns the logical name of an event (`name` for Event, `operation` for Run).
    pub fn name(&self) -> String {
        match self {
            TraceEvent::Event { name, .. } => name.clone(),
            TraceEvent::Run { operation } => operation.clone(),
        }
    }
}

thread_local! {
    static BUFFER: RefCell<Vec<TraceEvent>> = const { RefCell::new(Vec::new()) };
    static MOCKS: RefCell<HashMap<String, HashMap<String, String>>> =
        RefCell::new(HashMap::new());
}

/// Push an `Event { name, value }` onto the thread-local trace buffer.
pub fn emit_event(name: &str, value: &str) {
    BUFFER.with(|b| {
        b.borrow_mut().push(TraceEvent::Event {
            name: name.to_string(),
            value: value.to_string(),
        })
    });
}

/// Push a `Run { operation }` onto the thread-local trace buffer.
pub fn emit_run(operation: &str) {
    BUFFER.with(|b| {
        b.borrow_mut().push(TraceEvent::Run {
            operation: operation.to_string(),
        })
    });
}

/// Drain and return all accumulated trace events.
pub fn take_traces() -> Vec<TraceEvent> {
    BUFFER.with(|b| std::mem::take(&mut *b.borrow_mut()))
}

/// Clear traces and mock table — call at the start of each spec case.
pub fn reset() {
    BUFFER.with(|b| b.borrow_mut().clear());
    MOCKS.with(|m| m.borrow_mut().clear());
}

/// Install or replace the response table for `mock_name`.
pub fn set_mock(mock_name: &str, entries: &[(&str, &str)]) {
    let mut map = HashMap::new();
    for (k, v) in entries {
        map.insert((*k).to_string(), (*v).to_string());
    }
    MOCKS.with(|m| {
        m.borrow_mut().insert(mock_name.to_string(), map);
    });
}

/// Look up a configured mock response.
pub fn mock_lookup(mock_name: &str, input: &str) -> Option<String> {
    MOCKS.with(|m| {
        m.borrow()
            .get(mock_name)
            .and_then(|t| t.get(input).cloned())
    })
}

/// Implemented (typically via `#[derive(SpecEvent)]`) by structs that
/// expose annotated fields. Emits an `Event` per `#[spec_event]` field;
/// when `prefix` is `Some("alias")` the emitted names are `alias.field`.
pub trait SpecEvent {
    fn emit_fields(&self, prefix: Option<&str>);
}
