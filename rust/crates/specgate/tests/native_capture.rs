use specgate::{TraceEvent, Value, spec_component, spec_operation, spec_trace};

spec_component!("fixture.macro_default");

#[spec_operation("inner", spec = "fixture.macro_override")]
fn inner(value: i32) -> i32 {
    spec_trace!("seen", value);
    value * 2
}

#[spec_operation("outer")]
fn outer(value: i32) -> i32 {
    inner(value + 1) + 1
}

#[spec_operation("unit")]
fn unit() {}

fn config(operation_span_ids: &[&str]) -> specgate::__rt::NativeCaptureConfig {
    specgate::__rt::NativeCaptureConfig {
        scenario_name: "macro".to_string(),
        trace_id: "33333333333333333333333333333333".to_string(),
        run_span_id: "3333333333333301".to_string(),
        scenario_span_id: "3333333333333302".to_string(),
        operation_span_ids: operation_span_ids.iter().map(|id| (*id).to_string()).collect(),
        start_time_unix_nano: 3_000,
        clock_step_unix_nano: 10,
    }
}

#[test]
fn operation_macro_captures_components_nesting_results_and_observations() {
    specgate::__rt::reset();
    specgate::__rt::start_native_capture(config(&["3333333333333303", "3333333333333304"])).unwrap();

    assert_eq!(outer(2), 7);

    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert_eq!(capture.operations[0].component_id, "fixture.macro_default");
    assert_eq!(capture.operations[0].operation_name, "outer");
    assert_eq!(capture.operations[1].component_id, "fixture.macro_override");
    assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
    assert_eq!(capture.operations[0].inputs["value"], Value::Integer(2));
    assert_eq!(capture.operations[1].inputs["value"], Value::Integer(3));
    assert_eq!(capture.operations[1].observations[0].name, "seen");
    assert!(matches!(
        capture.operations[0].completion,
        Some(specgate::__rt::NativeCompletion::Result {
            value: Value::Integer(7),
            ..
        })
    ));

    assert_eq!(
        specgate::__rt::take_traces(),
        vec![
            TraceEvent::Run {
                operation: "outer".to_string(),
            },
            TraceEvent::Event {
                name: "outer.value".to_string(),
                value: Value::Integer(2),
            },
            TraceEvent::Run {
                operation: "inner".to_string(),
            },
            TraceEvent::Event {
                name: "inner.value".to_string(),
                value: Value::Integer(3),
            },
            TraceEvent::Event {
                name: "seen".to_string(),
                value: Value::Integer(3),
            },
            TraceEvent::Event {
                name: "$result".to_string(),
                value: Value::Integer(6),
            },
            TraceEvent::Event {
                name: "$result".to_string(),
                value: Value::Integer(7),
            },
        ]
    );
}

#[test]
fn operation_macro_is_compatible_without_capture_and_completes_unit_spans() {
    specgate::__rt::reset();
    assert_eq!(outer(2), 7);
    assert_eq!(specgate::__rt::take_traces().len(), 7);

    specgate::__rt::start_native_capture(config(&["3333333333333303"])).unwrap();
    unit();
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert_eq!(capture.operations[0].status, specgate::__rt::NativeStatus::Ok);
    assert!(capture.operations[0].completion.is_none());
}
