//! CTSC projection for `SpecGate` — translates legacy flat operation traces
//! into deterministic semantic CTSC format.
//!
//! `translate_legacy_trace` walks a JSON-encoded sequence of legacy
//! [`specgate_runtime::TraceEvent`]s — a leading `Run` event followed by
//! ordinary `Event`s — and re-projects it into a [`CtscProjection`]:
//!
//! - the leading `Run` event supplies `operation_name`;
//! - an event named `<operation_name>.<field>` is un-prefixed and becomes an
//!   operation input, keyed by `<field>`;
//! - every other ordinary event becomes an observation, keyed by its own name;
//! - the reserved `$result` / `$fault` event names select the terminal
//!   `completion` state (`"result"`, `"fault"`, or `"none"` if neither
//!   appears); that event's value becomes `completion_value_json`.
//!
//! Values keep their [`specgate_runtime::Value`] shape as-is: an event whose
//! value happens to look like `{"Integer": 7}` is a genuine single-entry map,
//! not a legacy tagged scalar, and is projected unchanged.

use serde::{Deserialize, Serialize};
use specgate::{SpecEvent, spec_component, spec_operation};
use specgate_runtime::{TraceEvent, Value};
use std::collections::BTreeMap;

spec_component!("specgate.ctsc");

#[derive(Debug, Clone, Serialize, Deserialize, SpecEvent)]
#[serde(rename_all = "snake_case")]
pub struct CtscProjection {
    #[spec_event]
    pub scenario_name: String,
    #[spec_event]
    pub component_id: String,
    #[spec_event]
    pub operation_name: String,
    #[spec_event]
    pub inputs_json: String,
    #[spec_event]
    pub observations_json: String,
    #[spec_event]
    pub completion: String,
    #[spec_event]
    pub completion_value_json: String,
}

/// The terminal legacy event, if any, that selects the CTSC completion state.
enum Completion {
    Result(Value),
    Fault(Value),
    None,
}

#[spec_operation("translate_legacy_trace")]
pub fn translate_legacy_trace(scenario_name: String, component_id: String, legacy_trace_json: String) -> CtscProjection {
    let trace_events: Vec<TraceEvent> = serde_json::from_str(&legacy_trace_json).expect("valid trace JSON required");

    let mut operation_name = String::new();
    let mut events_iter = trace_events.iter().peekable();

    if let Some(TraceEvent::Run { operation }) = events_iter.peek() {
        operation_name.clone_from(operation);
        events_iter.next();
    }

    let mut inputs_map: BTreeMap<String, Value> = BTreeMap::new();
    let mut observations_map: BTreeMap<String, Value> = BTreeMap::new();
    let mut completion_state = Completion::None;

    for event in events_iter {
        if let TraceEvent::Event { name, value } = event {
            if name == "$result" {
                completion_state = Completion::Result(value.clone());
            } else if name == "$fault" {
                completion_state = Completion::Fault(value.clone());
            } else if name.starts_with(&format!("{operation_name}.")) {
                let field_name = &name[operation_name.len() + 1..];
                inputs_map.insert(field_name.to_string(), value.clone());
            } else {
                observations_map.insert(name.clone(), value.clone());
            }
        }
    }

    let (completion, completion_value_json) = match completion_state {
        Completion::Result(value) => ("result".to_string(), to_json_string(&value)),
        Completion::Fault(value) => ("fault".to_string(), to_json_string(&value)),
        Completion::None => ("none".to_string(), String::new()),
    };

    CtscProjection {
        scenario_name,
        component_id,
        operation_name,
        inputs_json: to_json_string(&Value::Map(inputs_map)),
        observations_json: to_json_string(&Value::Map(observations_map)),
        completion,
        completion_value_json,
    }
}

/// Serialize a Value to compact JSON string using its Serialize impl.
fn to_json_string(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stateless_result() {
        let legacy_trace = r#"[{"kind":"Run","operation":"add"},{"kind":"Event","name":"add.a","value":2},{"kind":"Event","name":"add.b","value":3},{"kind":"Event","name":"$result","value":5}]"#;

        let result = translate_legacy_trace("add_2_3".to_string(), "fixture.stateless_add".to_string(), legacy_trace.to_string());

        assert_eq!(result.scenario_name, "add_2_3");
        assert_eq!(result.component_id, "fixture.stateless_add");
        assert_eq!(result.operation_name, "add");
        assert_eq!(result.inputs_json, r#"{"a":2,"b":3}"#);
        assert_eq!(result.observations_json, "{}");
        assert_eq!(result.completion, "result");
        assert_eq!(result.completion_value_json, "5");
    }

    #[test]
    fn observed_no_result() {
        let legacy_trace = r#"[{"kind":"Run","operation":"record_total"},{"kind":"Event","name":"record_total.amount","value":7},{"kind":"Event","name":"total","value":7}]"#;

        let result = translate_legacy_trace(
            "records_total".to_string(),
            "fixture.observation".to_string(),
            legacy_trace.to_string(),
        );

        assert_eq!(result.scenario_name, "records_total");
        assert_eq!(result.component_id, "fixture.observation");
        assert_eq!(result.operation_name, "record_total");
        assert_eq!(result.inputs_json, r#"{"amount":7}"#);
        assert_eq!(result.observations_json, r#"{"total":7}"#);
        assert_eq!(result.completion, "none");
        assert_eq!(result.completion_value_json, "");
    }

    #[test]
    fn unexpected_fault() {
        let legacy_trace = r#"[{"kind":"Run","operation":"explode"},{"kind":"Event","name":"explode.code","value":9},{"kind":"Event","name":"$fault","value":"boom"}]"#;

        let result = translate_legacy_trace("crashes".to_string(), "fixture.fault".to_string(), legacy_trace.to_string());

        assert_eq!(result.scenario_name, "crashes");
        assert_eq!(result.component_id, "fixture.fault");
        assert_eq!(result.operation_name, "explode");
        assert_eq!(result.inputs_json, r#"{"code":9}"#);
        assert_eq!(result.observations_json, "{}");
        assert_eq!(result.completion, "fault");
        assert_eq!(result.completion_value_json, r#""boom""#);
    }

    #[test]
    fn preserve_single_entry_map() {
        let legacy_trace = r#"[{"kind":"Run","operation":"record_metric"},{"kind":"Event","name":"metric","value":{"Integer":7}}]"#;

        let result = translate_legacy_trace(
            "records_metric".to_string(),
            "fixture.observation".to_string(),
            legacy_trace.to_string(),
        );

        assert_eq!(result.scenario_name, "records_metric");
        assert_eq!(result.component_id, "fixture.observation");
        assert_eq!(result.operation_name, "record_metric");
        assert_eq!(result.inputs_json, "{}");
        assert_eq!(result.observations_json, r#"{"metric":{"Integer":7}}"#);
        assert_eq!(result.completion, "none");
        assert_eq!(result.completion_value_json, "");
    }
}
