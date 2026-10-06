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

/// Suspends once, then records an observation and returns.
///
/// Used to prove that a future resumed on another thread still attaches its
/// observations to its own operation span.
#[spec_operation("async_traced")]
async fn async_traced(value: i32) -> i32 {
    PendingOnce::default().await;
    spec_trace!("seen", value);
    value * 2
}

/// Suspends once, probes the ambient session, then calls a nested operation.
///
/// The `start_native_capture` probe is the point: a resumed future must find
/// the collector handle already installed in the ambient slot. That is exactly
/// the short-circuit `activate_native_capture_from_environment` relies on, so a
/// thread that resumes a migrated future cannot start a second session writing
/// the same sidecar.
#[spec_operation("async_nests")]
async fn async_nests(value: i32) -> i32 {
    PendingOnce::default().await;
    let rejected = specgate::__rt::start_native_capture(config(&[]))
        .expect_err("a resumed future must find the session already installed in the ambient slot");
    spec_trace!("ambient_session", rejected);
    inner(value)
}

/// Suspends once, then panics, so a poll unwinds while a borrowed capture
/// context is installed in the resuming thread's ambient slot.
#[spec_operation("async_panics")]
async fn async_panics(value: i32) -> i32 {
    PendingOnce::default().await;
    panic!("async_panics unwound at {value}");
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
/// `#[spec_operation]` desugars an `async fn` so construction-time code can
/// capture the caller's operation as the future's parent, but the span itself
/// is still opened at first poll: building the future must leave the session
/// untouched, and a future driven to completion on the calling thread must
/// record exactly one operation with its inputs and result.
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

/// Two instrumented operations polled concurrently on one thread both record.
///
/// Interleaved operations do not nest: the first to finish is not the most
/// recently opened. Each instrumented future carries its own capture context
/// and re-installs it around every poll, so completion no longer depends on
/// open order and both operations record complete, correctly-parented traces.
///
/// This replaces a `#[should_panic]` test that pinned the old fail-closed
/// behavior. The reason that test existed — so a future change could not
/// silently record a *wrong* trace — is kept by asserting the trace is right:
/// distinct span IDs, both parented to the scenario, both `Ok`, both results
/// correct, and no residual outstanding scope at `finish_native_capture`.
#[test]
fn concurrently_interleaved_operations_record_correct_traces() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut context = Context::from_waker(Waker::noop());
    let mut first = Box::pin(async_suspends(2));
    let mut second = Box::pin(async_suspends(3));

    assert_eq!(first.as_mut().poll(&mut context), Poll::Pending);
    assert_eq!(second.as_mut().poll(&mut context), Poll::Pending);
    assert_eq!(first.as_mut().poll(&mut context), Poll::Ready(4));
    assert_eq!(second.as_mut().poll(&mut context), Poll::Ready(6));

    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert_eq!(capture.operations.len(), 2);
    assert_ne!(capture.operations[0].span_id, capture.operations[1].span_id);
    for (operation, expected) in capture.operations.iter().zip([2, 3]) {
        assert_eq!(operation.operation_name, "async_suspends");
        assert_eq!(operation.status, specgate::__rt::NativeStatus::Ok);
        assert_eq!(
            operation.parent_span_id, "3333333333333302",
            "neither operation nests inside the other; both are children of the scenario"
        );
        assert_eq!(operation.inputs["value"], Value::Integer(expected));
        assert!(matches!(
            operation.completion,
            Some(specgate::__rt::NativeCompletion::Result { value: Value::Integer(result), .. })
                if result == expected * 2
        ));
    }
}

/// Drive one future to completion with no executor, so poll order is the
/// test's own and span IDs stay deterministic.
fn block_on<F: Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, Waker};

    let mut pinned = Box::pin(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = pinned.as_mut().poll(&mut context) {
            return value;
        }
    }
}

/// A future records under the operation that *constructed* it, not under
/// whatever is current on the thread that resumes it.
///
/// `#[spec_operation]` desugars an `async fn` so there is construction-time
/// code: the capture context is taken while the caller's operation is still
/// current, and re-installed around every poll. Before that, a future resumed
/// on a thread with an empty ambient slot silently dropped its observations.
///
/// `std::thread::spawn` is used only as transport. Exactly one thread polls at
/// a time, so the recording order is fully determined.
#[test]
fn migrated_future_records_observations_under_its_construction_parent() {
    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut outer = specgate::__rt::begin_native_operation("fixture.macro_default", "outer").unwrap();
    outer.record_input("value", Value::Integer(2)).unwrap();

    let migrating = async_traced(2);
    let worker = std::thread::spawn(move || block_on(migrating));
    assert_eq!(worker.join().unwrap(), 4);

    outer.complete_result(Value::Integer(4)).unwrap();
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(capture.operations.len(), 2);
    assert_eq!(capture.operations[1].operation_name, "async_traced");
    assert_eq!(
        capture.operations[1].parent_span_id, capture.operations[0].span_id,
        "the migrated operation keeps the parent it was constructed under"
    );
    assert_eq!(capture.operations[1].observations.len(), 1);
    assert_eq!(capture.operations[1].observations[0].name, "seen");
    assert_eq!(capture.operations[1].observations[0].value, Value::Integer(2));
    assert_eq!(capture.operations[1].status, specgate::__rt::NativeStatus::Ok);
}

/// A nested operation reached from inside a migrated future is parented to the
/// migrated operation, and the resuming thread starts no second session.
///
/// The second claim is the dangerous one: a thread with an empty ambient slot
/// and `SPECGATE_NATIVE_CAPTURE` set would start an independent session writing
/// the same sidecar path, silently clobbering the recording. The body asserts
/// the slot is already occupied on the resuming thread, which is the exact
/// condition that short-circuits that activation.
#[test]
fn nested_operation_inside_a_migrated_future_is_parented_and_starts_no_second_session() {
    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut outer = specgate::__rt::begin_native_operation("fixture.macro_default", "outer").unwrap();

    let migrating = async_nests(2);
    let worker = std::thread::spawn(move || block_on(migrating));
    assert_eq!(worker.join().unwrap(), 4);

    outer.complete_result(Value::Integer(4)).unwrap();
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(
        capture
            .operations
            .iter()
            .map(|operation| operation.operation_name.as_str())
            .collect::<Vec<_>>(),
        ["outer", "async_nests", "inner"],
        "one session recorded all three operations"
    );
    assert_eq!(capture.operations[1].parent_span_id, capture.operations[0].span_id);
    assert_eq!(
        capture.operations[2].parent_span_id, capture.operations[1].span_id,
        "the nested operation is parented to the migrated async operation"
    );
    assert_eq!(capture.operations[2].component_id, "fixture.macro_override");
    assert_eq!(
        capture.operations[1].observations[0].value,
        Value::String("a native capture session is already active".to_string()),
        "the resuming thread found the collector handle already installed"
    );
}

/// A panicking poll restores the resuming thread's ambient slot.
///
/// `InstalledContext::Drop` is the only thing standing between an unwinding
/// poll and another session's collector handle being stranded in this thread's
/// thread-local slot. A stranded handle would make every later operation on
/// that thread record into a session that is already finished. Every other
/// async test here completes normally, so without this one the restore is
/// verified only by reading the code.
#[test]
fn a_panicking_poll_restores_the_ambient_slot_on_the_resuming_thread() {
    specgate::__rt::start_native_capture(config(&[])).unwrap();
    let mut outer = specgate::__rt::begin_native_operation("fixture.macro_default", "outer").unwrap();

    let migrating = async_panics(2);
    let worker = std::thread::spawn(move || {
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || block_on(migrating)));
        // The borrowed handle must be gone: starting a session here can only
        // succeed if this thread's slot is empty again.
        let restored = specgate::__rt::start_native_capture(config(&[]));
        let probe = specgate::__rt::finish_native_capture();
        (unwound.is_err(), restored, probe)
    });

    let (unwound, restored, probe) = worker.join().unwrap();
    assert!(unwound, "the body panicked, so the poll unwound");
    restored.expect("the unwinding poll restored the resuming thread's ambient slot");
    probe.expect("the probe session on the resuming thread is independent and finishes clean");

    outer.complete_result(Value::Integer(0)).unwrap();
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(capture.operations.len(), 2);
    assert_eq!(capture.operations[1].operation_name, "async_panics");
    assert_eq!(capture.operations[1].status, specgate::__rt::NativeStatus::Error);
    assert!(
        matches!(
            &capture.operations[1].completion,
            Some(specgate::__rt::NativeCompletion::Fault { fault_type, .. })
                if fault_type == "specgate.unexpected_target_fault"
        ),
        "the unwound operation is recorded as a target fault, not silently dropped"
    );
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
