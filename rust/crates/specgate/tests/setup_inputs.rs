use specgate::{SpecEvent, Value, spec_component, spec_operation, spec_setup};
use std::collections::BTreeMap;

spec_component!("fixture.setup_inputs");

#[derive(Debug, SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment")]
pub fn make_counter(#[spec_input("initial")] start: i32) -> Counter {
    Counter { count: start }
}

impl Counter {
    #[spec_operation("increment")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}

#[derive(Debug, SpecEvent)]
pub struct BoxVal {
    #[spec_event]
    pub value: i32,
}

#[spec_setup("combine", fills = "left")]
#[spec_setup("combine", fills = "right")]
pub fn make_box() -> BoxVal {
    BoxVal { value: 2 }
}

#[spec_operation("combine")]
pub fn combine(left: &BoxVal, right: &BoxVal, bonus: i32) -> i32 {
    left.value + right.value + bonus
}

#[derive(Debug, SpecEvent)]
pub struct Factor {
    #[spec_event]
    pub factor: i32,
}

#[spec_setup("scale", fills = "factor")]
pub fn make_factor(#[spec_input("factor_seed")] seed: i32) -> Factor {
    Factor { factor: seed }
}

#[spec_operation("scale")]
pub fn scale(factor: &Factor, value: i32) -> i32 {
    factor.factor * value
}

#[spec_setup("double_it")]
pub fn seed_value() -> i32 {
    21
}

#[spec_operation("double_it")]
pub fn double_it(value: i32) -> i32 {
    value * 2
}

#[derive(Debug, SpecEvent)]
pub struct FragileCounter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("inspect_fragile")]
pub fn make_fragile_counter(#[spec_input("initial")] start: i32) -> FragileCounter {
    panic!("setup boom for {start}");
}

impl FragileCounter {
    #[spec_operation("inspect_fragile")]
    pub fn inspect_fragile(&self) -> i32 {
        self.count
    }
}

#[derive(Debug, SpecEvent)]
pub struct State {
    #[spec_event]
    pub seed: i32,
}

#[spec_setup("first")]
#[spec_setup("second")]
pub fn make_state(#[spec_input("seed")] value: i32) -> State {
    State { seed: value }
}

impl State {
    #[spec_operation("first")]
    pub fn first(&self) -> i32 {
        self.seed
    }

    #[spec_operation("second")]
    pub fn second(&self) -> i32 {
        self.seed * 2
    }
}

#[spec_operation("increment", spec = "fixture.setup_inputs.other")]
pub fn other_increment(amount: i32) -> i32 {
    amount + 1
}

fn config() -> specgate::__rt::NativeCaptureConfig {
    specgate::__rt::NativeCaptureConfig {
        scenario_name: "setup-inputs".to_string(),
        trace_id: "44444444444444444444444444444444".to_string(),
        run_span_id: "4444444444444401".to_string(),
        scenario_span_id: "4444444444444402".to_string(),
        operation_span_ids: Vec::new(),
        start_time_unix_nano: 4_000,
        clock_step_unix_nano: 10,
    }
}

fn inputs_of(capture: &specgate::__rt::NativeCapture, operation: &str) -> BTreeMap<String, Value> {
    capture
        .operations
        .iter()
        .find(|span| span.operation_name == operation)
        .unwrap_or_else(|| panic!("no captured operation named '{operation}'"))
        .inputs
        .clone()
}

fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = panic.downcast_ref::<&str>() {
        return (*message).to_string();
    }
    "<non-string panic>".to_string()
}

#[test]
fn receiver_setup_construction_inputs_become_operation_inputs() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let mut counter = make_counter(4);
    counter.increment();
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(counter.count, 5);
    assert_eq!(
        inputs_of(&capture, "increment"),
        BTreeMap::from([("initial".to_string(), Value::Integer(4))])
    );
}

#[test]
fn stacked_fills_omit_every_setup_filled_parameter() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let left = make_box();
    let right = make_box();
    assert_eq!(combine(&left, &right, 5), 9);
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(
        inputs_of(&capture, "combine"),
        BTreeMap::from([("bonus".to_string(), Value::Integer(5))]),
        "parameters a setup constructs are folded away, the rest stay"
    );
}

#[test]
fn explicit_fills_replaces_its_parameter_with_the_setup_inputs() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let factor = make_factor(3);
    assert_eq!(scale(&factor, 7), 21);
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(
        inputs_of(&capture, "scale"),
        BTreeMap::from([
            ("factor_seed".to_string(), Value::Integer(3)),
            ("value".to_string(), Value::Integer(7)),
        ])
    );
}

#[test]
fn unset_fills_resolves_the_parameter_by_setup_return_type() {
    specgate::__rt::start_native_capture(config()).unwrap();
    assert_eq!(double_it(seed_value()), 42);
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(inputs_of(&capture, "double_it"), BTreeMap::new());
}

#[test]
fn missing_zero_input_setup_provenance_fails_before_suppressing_a_filled_parameter() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let panic = std::panic::catch_unwind(|| {
        let _ = double_it(21);
    })
    .expect_err("capture must reject a folded setup parameter with no successful setup provenance");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("failed to begin native operation")
            && message.contains("fixture.setup_inputs::double_it")
            && message.contains("seed_value"),
        "the panic must name the folded operation and missing setup: {message}"
    );

    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(capture.operations.is_empty());
}

#[test]
fn a_panicking_setup_leaves_no_provenance_for_the_following_operation() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let setup_panic = std::panic::catch_unwind(|| {
        let _ = make_fragile_counter(7);
    })
    .expect_err("the setup body should panic");
    assert_eq!(panic_message(setup_panic.as_ref()), "setup boom for 7");

    let counter = FragileCounter { count: 7 };
    let operation_panic = std::panic::catch_unwind(|| {
        let _ = counter.inspect_fragile();
    })
    .expect_err("a failed setup must not leave stale provenance behind");
    let message = panic_message(operation_panic.as_ref());
    assert!(
        message.contains("failed to begin native operation")
            && message.contains("fixture.setup_inputs::inspect_fragile")
            && message.contains("make_fragile_counter"),
        "the following operation must fail for the missing successful setup: {message}"
    );

    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(capture.operations.is_empty());
}

#[test]
fn stacked_setups_attribute_named_inputs_to_each_declared_operation() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let state = make_state(6);
    assert_eq!(state.first(), 6);
    assert_eq!(state.second(), 12);
    let capture = specgate::__rt::finish_native_capture().unwrap();

    let expected = BTreeMap::from([("seed".to_string(), Value::Integer(6))]);
    assert_eq!(inputs_of(&capture, "first"), expected);
    assert_eq!(inputs_of(&capture, "second"), expected);
}

#[test]
fn setup_inputs_never_cross_a_component_boundary() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let _counter = make_counter(4);
    assert_eq!(other_increment(1), 2);
    let capture = specgate::__rt::finish_native_capture().unwrap();

    let span = &capture.operations[0];
    assert_eq!(span.component_id, "fixture.setup_inputs.other");
    assert_eq!(span.inputs, BTreeMap::from([("amount".to_string(), Value::Integer(1))]));
}

#[test]
fn operations_and_setups_outside_a_capture_session_still_behave_normally() {
    let mut counter = make_counter(4);
    counter.increment();
    assert_eq!(counter.count, 5);
    assert_eq!(double_it(21), 42);

    let left = make_box();
    let right = make_box();
    assert_eq!(combine(&left, &right, 5), 9);
}

#[test]
fn finishing_a_session_clears_setup_provenance_for_the_next_one() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let mut counter = make_counter(4);
    counter.increment();
    specgate::__rt::finish_native_capture().unwrap();

    specgate::__rt::start_native_capture(config()).unwrap();
    let panic = std::panic::catch_unwind(move || {
        counter.increment();
    })
    .expect_err("a new session must require a fresh successful setup");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("failed to begin native operation")
            && message.contains("fixture.setup_inputs::increment")
            && message.contains("make_counter"),
        "the new session must not inherit old setup provenance: {message}"
    );
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert!(capture.operations.is_empty());
}
#[test]
fn repeating_one_setup_with_identical_inputs_stays_unambiguous() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let first = make_counter(4);
    let mut second = make_counter(4);
    second.increment();
    let capture = specgate::__rt::finish_native_capture().unwrap();

    assert_eq!(first.count, 4);
    assert_eq!(second.count, 5);
    assert_eq!(
        inputs_of(&capture, "increment"),
        BTreeMap::from([("initial".to_string(), Value::Integer(4))]),
        "two identical constructions describe one unambiguous public input surface"
    );
}

/// Capture cannot associate a returned receiver with the construction that
/// produced it, so two differing constructions of one setup declaration are
/// ambiguous. The annotation turns the rejection into a panic at the second
/// construction rather than misattributing the later values.
#[test]
fn repeating_one_setup_with_different_inputs_fails_the_scenario() {
    specgate::__rt::start_native_capture(config()).unwrap();
    let panic = std::panic::catch_unwind(|| {
        let _first = make_counter(4);
        let _second = make_counter(9);
    })
    .expect_err("a differing repeat construction must not be silently accepted");
    let message = panic_message(panic.as_ref());
    assert!(
        message.contains("ran twice in one capture with different inputs")
            && message.contains("initial=Integer(4) then initial=Integer(9)"),
        "the panic must name both constructions: {message}"
    );

    let capture = specgate::__rt::finish_native_capture().unwrap();
    assert!(capture.operations.is_empty());
}
