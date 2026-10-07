//! Final capture projection, deterministic sidecar serialization, and validation.

use super::{
    BTreeMap, Capture, CaptureError, Completion, ComponentId, Config, HashSet, Observation, OperationName, OperationSpan, PendingOperation,
    Serialize, SpanBoundary, SpanId, State, Status, TraceId, Value,
};

#[derive(Serialize)]
pub(super) struct CaptureSnapshot<'a> {
    trace_id: &'a TraceId,
    scenario_name: &'a str,
    run: SpanSnapshot<'a>,
    scenario: SpanSnapshot<'a>,
    operations: OperationsSnapshot<'a>,
}

#[derive(Serialize)]
struct SpanSnapshot<'a> {
    span_id: &'a SpanId,
    parent_span_id: Option<&'a SpanId>,
    #[serde(rename = "start_time_unix_nano")]
    start_ns: i64,
    #[serde(rename = "end_time_unix_nano")]
    end_ns: i64,
    status: Status,
}

struct OperationsSnapshot<'a>(&'a [PendingOperation]);

impl Serialize for OperationsSnapshot<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeSeq as _;
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for operation in self.0 {
            sequence.serialize_element(&OperationSnapshot {
                order: operation.order,
                span_id: &operation.span_id,
                parent_span_id: &operation.parent_span_id,
                component_id: &operation.component_id,
                operation_name: &operation.operation_name,
                start_ns: operation.start_ns,
                end_ns: operation.end_ns.expect("completed capture operation must have an end timestamp"),
                status: operation.status.expect("completed capture operation must have terminal status"),
                inputs: &operation.inputs,
                observations: &operation.observations,
                completion: &operation.completion,
            })?;
        }
        sequence.end()
    }
}

#[derive(Serialize)]
struct OperationSnapshot<'a> {
    order: u64,
    span_id: &'a SpanId,
    parent_span_id: &'a SpanId,
    component_id: &'a ComponentId,
    operation_name: &'a OperationName,
    #[serde(rename = "start_time_unix_nano")]
    start_ns: i64,
    #[serde(rename = "end_time_unix_nano")]
    end_ns: i64,
    status: Status,
    #[serde(serialize_with = "crate::value::wire::map::serialize_ref")]
    inputs: &'a BTreeMap<String, Value>,
    observations: &'a [Observation],
    completion: &'a Option<Completion>,
}

impl<'a> CaptureSnapshot<'a> {
    pub(super) fn new(state: &'a State) -> Result<Self, CaptureError> {
        assert!(
            state.active_ops.is_empty(),
            "capture snapshot requires every operation scope to be completed"
        );
        if let Some(error) = &state.terminal_error {
            return Err(error.as_error());
        }
        let scenario_end_ns = state.next_time_ns;
        let run_end_ns = scenario_end_ns
            .checked_add(state.config.step_ns)
            .ok_or_else(|| "native capture logical clock overflow".to_string())?;
        let status = if state.operations.iter().any(|operation| operation.status == Some(Status::Error)) {
            Status::Error
        } else {
            Status::Ok
        };
        Ok(Self {
            trace_id: &state.config.trace_id,
            scenario_name: &state.config.scenario_name,
            run: SpanSnapshot {
                span_id: &state.config.run_span_id,
                parent_span_id: None,
                start_ns: state.run_start_ns,
                end_ns: run_end_ns,
                status,
            },
            scenario: SpanSnapshot {
                span_id: &state.config.scenario_span_id,
                parent_span_id: Some(&state.config.run_span_id),
                start_ns: state.scenario_start,
                end_ns: scenario_end_ns,
                status,
            },
            operations: OperationsSnapshot(&state.operations),
        })
    }
}

pub(super) fn build(state: &State) -> Result<Capture, CaptureError> {
    assert!(
        state.active_ops.is_empty(),
        "capture snapshot requires every operation scope to be completed"
    );
    if let Some(error) = &state.terminal_error {
        return Err(error.as_error());
    }
    let scenario_end_ns = state.next_time_ns;
    let run_end_ns = scenario_end_ns
        .checked_add(state.config.step_ns)
        .ok_or_else(|| "native capture logical clock overflow".to_string())?;
    let has_error = state.operations.iter().any(|operation| operation.status == Some(Status::Error));
    let operations = state
        .operations
        .iter()
        .map(|operation| OperationSpan {
            order: operation.order,
            span_id: operation.span_id.clone(),
            parent_id: operation.parent_span_id.clone(),
            component_id: operation.component_id.clone(),
            operation_name: operation.operation_name.clone(),
            start_ns: operation.start_ns,
            end_ns: operation.end_ns.expect("completed capture operation must have an end timestamp"),
            status: operation.status.expect("completed capture operation must have terminal status"),
            inputs: operation.inputs.clone(),
            observations: operation.observations.clone(),
            completion: operation.completion.clone(),
        })
        .collect::<Vec<_>>();
    let root_status = if has_error { Status::Error } else { Status::Ok };
    Ok(Capture {
        trace_id: state.config.trace_id.clone(),
        scenario_name: state.config.scenario_name.clone(),
        run: SpanBoundary {
            span_id: state.config.run_span_id.clone(),
            parent_id: None,
            start_ns: state.run_start_ns,
            end_ns: run_end_ns,
            status: root_status,
        },
        scenario: SpanBoundary {
            span_id: state.config.scenario_span_id.clone(),
            parent_id: Some(state.config.run_span_id.clone()),
            start_ns: state.scenario_start,
            end_ns: scenario_end_ns,
            status: root_status,
        },
        operations,
    })
}

pub(super) fn validate_config(config: &Config) -> Result<(), CaptureError> {
    if config.start_ns < 0 {
        return Err("native capture start timestamp must be non-negative".to_string().into());
    }
    if config.step_ns <= 0 {
        return Err("native capture logical clock step must be positive".to_string().into());
    }
    let mut span_ids = HashSet::with_capacity(config.operation_ids.len() + 2);
    span_ids.insert(config.run_span_id.as_str());
    if !span_ids.insert(config.scenario_span_id.as_str()) {
        return Err("native capture span IDs must be unique".to_string().into());
    }
    for (index, span_id) in config.operation_ids.iter().enumerate() {
        if !span_ids.insert(span_id.as_str()) {
            return Err(format!("native capture operation span ID at index {index} is duplicated").into());
        }
    }
    Ok(())
}

pub(super) fn validate_hex_id(label: impl AsRef<str>, value: impl AsRef<str>, length: usize) -> Result<(), CaptureError> {
    let label = label.as_ref();
    let value = value.as_ref();
    if value.len() != length
        || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || value.bytes().all(|byte| byte == b'0')
    {
        return Err(format!("{label} must be a non-zero {length}-character lowercase hexadecimal string").into());
    }
    Ok(())
}
