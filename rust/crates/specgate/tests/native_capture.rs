use specgate::{SpecEvent, ToNativeValue, Value, spec_component, spec_operation, spec_trace};

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

#[spec_operation("optional")]
fn optional(value: Option<String>) -> Option<String> {
    value
}

#[spec_operation("async_value")]
async fn async_value(value: i32) -> i32 {
    value * 2
}

#[spec_operation("async_suspends")]
async fn async_suspends(value: i32) -> i32 {
    PendingOnce::default().await;
    value * 2
}

/// A future that yields exactly once, so two of them can be interleaved.
#[derive(Default)]
struct PendingOnce {
    polled: bool,
}

impl Future for PendingOnce {
    type Output = ();

    fn poll(mut self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<()> {
        if self.polled {
            std::task::Poll::Ready(())
        } else {
            self.polled = true;
            context.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }
}

#[spec_operation("result_unit")]
fn result_unit(fail: bool) -> Result<(), String> {
    if fail { Err("failed".to_string()) } else { Ok(()) }
}

#[spec_operation("option_unit")]
fn option_unit(present: bool) -> Option<()> {
    present.then_some(())
}

#[spec_operation("result_error_unit")]
fn result_error_unit(fail: bool) -> Result<i32, ()> {
    if fail { Err(()) } else { Ok(7) }
}

#[spec_operation("result_both_unit")]
fn result_both_unit(fail: bool) -> Result<(), ()> {
    if fail { Err(()) } else { Ok(()) }
}

#[derive(Clone, SpecEvent)]
struct OptionalRecord {
    #[spec_event]
    values: Vec<Option<String>>,
}

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
}

#[test]
fn operation_macro_is_compatible_without_capture_and_completes_unit_spans() {
    assert_eq!(outer(2), 7);

    specgate::__rt::start_native_capture(config(&["3333333333333303"])).unwrap();
    unit();
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert_eq!(capture.operations[0].status, specgate::__rt::NativeStatus::Ok);
    assert!(capture.operations[0].completion.is_none());
}

#[test]
fn operation_macro_captures_native_optional_values() {
    for (input, native_variant) in [(Some("alice".to_string()), "Some"), (None, "None")] {
        specgate::__rt::start_native_capture(config(&["3333333333333303"])).unwrap();
        assert_eq!(optional(input.clone()), input);

        let capture = specgate::__rt::finish_native_capture().unwrap();
        let Value::Map(native_input) = &capture.operations[0].inputs["value"] else {
            panic!("native optional input must be a kvlist");
        };
        assert_eq!(native_input.len(), 1);
        assert!(native_input.contains_key(native_variant));
        match (&input, &capture.operations[0].completion) {
            (Some(_), Some(specgate::__rt::NativeCompletion::Result { value, .. })) => {
                assert_eq!(value, native_input.get("Some").unwrap());
            }
            (None, Some(specgate::__rt::NativeCompletion::Empty { .. })) => {}
            _ => panic!("unexpected optional completion"),
        }
    }
}

#[test]
fn native_projection_recurses_through_collections_and_annotated_records() {
    let value = OptionalRecord {
        values: vec![Some("alice".to_string()), None],
    };

    assert_eq!(
        value.to_native_value(),
        Value::Map(std::collections::BTreeMap::from([(
            "values".to_string(),
            Value::List(vec![
                Value::Map(std::collections::BTreeMap::from([(
                    "Some".to_string(),
                    Value::String("alice".to_string()),
                )])),
                Value::Map(std::collections::BTreeMap::from([(
                    "None".to_string(),
                    Value::Map(std::collections::BTreeMap::new()),
                )])),
            ]),
        )]))
    );
}

/// Recording begins at first poll, not at construction.
///
/// An `async fn` has no construction-time code, so building the future must
/// leave the session untouched; a future that is driven to completion on the
/// calling thread must record exactly one operation with its inputs and result.
#[test]
fn async_operation_records_from_first_poll_through_completion() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    drop(async_value(2));
    assert!(
        specgate::__rt::finish_native_capture().unwrap().operations.is_empty(),
        "constructing a future must not open an operation scope"
    );

    let mut context = Context::from_waker(Waker::noop());
    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut captured = Box::pin(async_value(2));
    assert_eq!(captured.as_mut().poll(&mut context), Poll::Ready(4));
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert_eq!(capture.operations.len(), 1);
    assert_eq!(capture.operations[0].operation_name, "async_value");
    assert_eq!(
        capture.operations[0].inputs,
        std::collections::BTreeMap::from([("value".to_string(), Value::Integer(2))])
    );
    assert!(matches!(
        capture.operations[0].completion,
        Some(specgate::__rt::NativeCompletion::Result {
            value: Value::Integer(4),
            ..
        })
    ));
}

/// Two instrumented operations polled concurrently on one thread fail closed.
///
/// `state.active_operations` in `specgate-runtime` is a single nesting stack,
/// which assumes operations nest like function calls. Interleaved futures do
/// not nest: the first operation to finish is no longer on top of the stack, so
/// completion is rejected and the run is poisoned. No thread migration and no
/// abandonment are involved.
///
/// This is expected, not a bug. Concurrent interleaving is unsupported, and
/// failing loudly is the correct response. The test pins that it keeps failing
/// loudly, so a future change cannot silently record a wrong trace instead.
#[test]
#[should_panic(expected = "attempted completion while a nested scope is active")]
fn concurrently_interleaved_operations_fail_closed() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut context = Context::from_waker(Waker::noop());
    let mut first = Box::pin(async_suspends(2));
    let mut second = Box::pin(async_suspends(3));

    assert_eq!(first.as_mut().poll(&mut context), Poll::Pending);
    assert_eq!(second.as_mut().poll(&mut context), Poll::Pending);
    let _ = first.as_mut().poll(&mut context);
}

#[test]
fn unit_result_and_optional_unit_use_consistent_ctsc_completion() {
    specgate::__rt::start_native_capture(config(&[])).unwrap();
    assert_eq!(result_unit(false), Ok(()));
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(capture.operations[0].completion.is_none());

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    assert_eq!(result_unit(true), Err("failed".to_string()));
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(matches!(
        capture.operations[0].completion,
        Some(specgate::__rt::NativeCompletion::Error {
            value: Some(Value::String(_)),
            ..
        })
    ));

    for present in [true, false] {
        specgate::__rt::start_native_capture(config(&[])).unwrap();
        assert_eq!(option_unit(present), present.then_some(()));
        let capture = specgate::__rt::finish_native_capture().unwrap();
        assert!(matches!(
            capture.operations[0].completion,
            Some(specgate::__rt::NativeCompletion::Result { value: Value::Map(_), .. })
        ));
    }

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    assert_eq!(result_error_unit(true), Err(()));
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(matches!(
        capture.operations[0].completion,
        Some(specgate::__rt::NativeCompletion::Error { value: None, .. })
    ));

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    assert_eq!(result_both_unit(false), Ok(()));
    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(capture.operations[0].completion.is_none());
}
