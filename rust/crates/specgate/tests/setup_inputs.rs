//! Setup-input folding integration fixtures and tests.

use specgate::{SpecEvent, Value, spec_component, spec_operation, spec_setup};
use std::collections::BTreeMap;

spec_component!("fixture.setup_inputs");

/// Mutable receiver used to verify setup construction input folding.
#[derive(Debug, Clone, Copy, SpecEvent)]
#[expect(
    clippy::exhaustive_structs,
    reason = "the public fixture shape is intentionally fixed for discovery"
)]
pub struct Counter {
    /// Current counter value.
    #[spec_event]
    pub count: i32,
}

/// Construct a counter for the `increment` operation.
#[spec_setup("increment")]
pub fn make_counter(#[spec_input("initial")] start: i32) -> Counter {
    Counter { count: start }
}

impl Counter {
    /// Increment the captured receiver.
    #[spec_operation("increment")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}

/// Boxed integer fixture used for explicit setup fills.
#[derive(Debug, Clone, Copy, SpecEvent)]
#[expect(
    clippy::exhaustive_structs,
    reason = "the public fixture shape is intentionally fixed for discovery"
)]
pub struct BoxVal {
    /// Boxed integer value.
    #[spec_event]
    pub value: i32,
}

/// Construct a boxed value for either `combine` input.
#[spec_setup("combine", fills = "left")]
#[spec_setup("combine", fills = "right")]
pub fn make_box() -> BoxVal {
    BoxVal { value: 2 }
}

/// Combine two setup-filled values and one ordinary input.
#[spec_operation("combine")]
pub fn combine(left: &BoxVal, right: &BoxVal, bonus: i32) -> i32 {
    left.value + right.value + bonus
}

/// Multiplication factor fixture used for an explicitly named fill.
#[derive(Debug, Clone, Copy, SpecEvent)]
#[expect(
    clippy::exhaustive_structs,
    reason = "the public fixture shape is intentionally fixed for discovery"
)]
pub struct Factor {
    /// Multiplication factor.
    #[spec_event]
    pub factor: i32,
}

/// Construct a factor from a renamed setup input.
#[spec_setup("scale", fills = "factor")]
pub fn make_factor(#[spec_input("factor_seed")] seed: i32) -> Factor {
    Factor { factor: seed }
}

/// Multiply an ordinary input by a setup-filled factor.
#[spec_operation("scale")]
pub fn scale(factor: &Factor, value: i32) -> i32 {
    factor.factor * value
}

/// Supply the zero-input setup value for `double_it`.
#[spec_setup("double_it")]
pub fn seed_value() -> i32 {
    21
}

/// Double a setup-filled integer.
#[spec_operation("double_it")]
pub fn double_it(value: i32) -> i32 {
    value * 2
}

/// Receiver whose setup intentionally panics.
#[derive(Debug, Clone, Copy, SpecEvent)]
#[expect(
    clippy::exhaustive_structs,
    reason = "the public fixture shape is intentionally fixed for discovery"
)]
pub struct FragileCounter {
    /// Current counter value.
    #[spec_event]
    pub count: i32,
}

/// Panic while attempting to construct a fragile counter.
///
/// # Panics
///
/// Always panics so capture can verify failed setup provenance.
#[spec_setup("inspect_fragile")]
pub fn make_fragile_counter(#[spec_input("initial")] start: i32) -> FragileCounter {
    panic!("setup boom for {start}");
}

impl FragileCounter {
    /// Return the counter value after a failed setup.
    #[spec_operation("inspect_fragile")]
    pub fn inspect_fragile(&self) -> i32 {
        self.count
    }
}

/// Shared receiver fixture for two stacked setup declarations.
#[derive(Debug, Clone, Copy, SpecEvent)]
#[expect(
    clippy::exhaustive_structs,
    reason = "the public fixture shape is intentionally fixed for discovery"
)]
pub struct State {
    /// Seed shared by both operations.
    #[spec_event]
    pub seed: i32,
}

/// Construct state for both stacked operations.
#[spec_setup("first")]
#[spec_setup("second")]
pub fn make_state(#[spec_input("seed")] value: i32) -> State {
    State { seed: value }
}

impl State {
    /// Return the original seed.
    #[spec_operation("first")]
    pub fn first(&self) -> i32 {
        self.seed
    }

    /// Return twice the original seed.
    #[spec_operation("second")]
    pub fn second(&self) -> i32 {
        self.seed * 2
    }
}

/// Increment an input in a different component.
#[spec_operation("increment", spec = "fixture.setup_inputs.other")]
pub fn other_increment(amount: i32) -> i32 {
    amount + 1
}

fn config() -> specgate::__rt::Config {
    specgate::__rt::Config::builder(specgate::__rt::ConfigDeps {
        scenario_name: "setup-inputs".into(),
        trace_id: "44444444444444444444444444444444".try_into().unwrap(),
        run_id: "4444444444444401".try_into().unwrap(),
        scenario_id: "4444444444444402".try_into().unwrap(),
    })
    .start_time(4_000)
    .clock_step(10)
    .build()
    .unwrap()
}

fn inputs_of(capture: &specgate::__rt::Capture, operation: &str) -> BTreeMap<String, Value> {
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
    specgate::__rt::start(config()).unwrap();
    let mut counter = make_counter(4);
    counter.increment();
    let capture = specgate::__rt::finish().unwrap();

    assert_eq!(counter.count, 5);
    assert_eq!(
        inputs_of(&capture, "increment"),
        BTreeMap::from([("initial".to_string(), Value::Integer(4))])
    );
}

#[test]
fn stacked_fills_omit_every_setup_filled_parameter() {
    specgate::__rt::start(config()).unwrap();
    let left = make_box();
    let right = make_box();
    assert_eq!(combine(&left, &right, 5), 9);
    let capture = specgate::__rt::finish().unwrap();

    assert_eq!(
        inputs_of(&capture, "combine"),
        BTreeMap::from([("bonus".to_string(), Value::Integer(5))]),
        "parameters a setup constructs are folded away, the rest stay"
    );
}

#[test]
fn explicit_fills_replaces_its_parameter_with_the_setup_inputs() {
    specgate::__rt::start(config()).unwrap();
    let factor = make_factor(3);
    assert_eq!(scale(&factor, 7), 21);
    let capture = specgate::__rt::finish().unwrap();

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
    specgate::__rt::start(config()).unwrap();
    assert_eq!(double_it(seed_value()), 42);
    let capture = specgate::__rt::finish().unwrap();

    assert_eq!(inputs_of(&capture, "double_it"), BTreeMap::new());
}

#[test]
fn missing_zero_input_setup_provenance_fails_before_suppressing_a_filled_parameter() {
    specgate::__rt::start(config()).unwrap();
    assert_eq!(double_it(21), 42, "capture failure must not alter target behavior");
    let message = specgate::__rt::finish().expect_err("capture must reject a folded setup parameter with no successful setup provenance");
    assert!(
        message.contains("fixture.setup_inputs::double_it") && message.contains("seed_value"),
        "the capture failure must name the folded operation and missing setup: {message}"
    );
}

#[test]
fn a_panicking_setup_leaves_no_provenance_for_the_following_operation() {
    specgate::__rt::start(config()).unwrap();
    let setup_panic = std::panic::catch_unwind(|| {
        let _ = make_fragile_counter(7);
    })
    .expect_err("the setup body should panic");
    assert_eq!(panic_message(setup_panic.as_ref()), "setup boom for 7");

    let counter = FragileCounter { count: 7 };
    assert_eq!(counter.inspect_fragile(), 7, "capture failure must preserve the target result");
    let message = specgate::__rt::finish().expect_err("a failed setup must not leave stale provenance behind");
    assert!(
        message.contains("fixture.setup_inputs::inspect_fragile") && message.contains("make_fragile_counter"),
        "capture must fail for the missing successful setup: {message}"
    );
}

#[test]
fn stacked_setups_attribute_named_inputs_to_each_declared_operation() {
    specgate::__rt::start(config()).unwrap();
    let state = make_state(6);
    assert_eq!(state.first(), 6);
    assert_eq!(state.second(), 12);
    let capture = specgate::__rt::finish().unwrap();

    let expected = BTreeMap::from([("seed".to_string(), Value::Integer(6))]);
    assert_eq!(inputs_of(&capture, "first"), expected);
    assert_eq!(inputs_of(&capture, "second"), expected);
}

#[test]
fn setup_inputs_never_cross_a_component_boundary() {
    specgate::__rt::start(config()).unwrap();
    let _counter = make_counter(4);
    assert_eq!(other_increment(1), 2);
    let capture = specgate::__rt::finish().unwrap();

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
    specgate::__rt::start(config()).unwrap();
    let mut counter = make_counter(4);
    counter.increment();
    specgate::__rt::finish().unwrap();

    specgate::__rt::start(config()).unwrap();
    counter.increment();
    let message = specgate::__rt::finish().expect_err("a new session must require a fresh successful setup");
    assert!(
        message.contains("fixture.setup_inputs::increment") && message.contains("make_counter"),
        "the new session must not inherit old setup provenance: {message}"
    );
}
#[test]
fn repeating_one_setup_with_identical_inputs_stays_unambiguous() {
    specgate::__rt::start(config()).unwrap();
    let first = make_counter(4);
    let mut second = make_counter(4);
    second.increment();
    let capture = specgate::__rt::finish().unwrap();

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
    specgate::__rt::start(config()).unwrap();
    let _first = make_counter(4);
    let _second = make_counter(9);
    let message = specgate::__rt::finish().expect_err("a differing repeat construction must not be silently accepted");
    assert!(
        message.contains("ran twice in one capture with different inputs")
            && message.contains("initial=Integer(4) then initial=Integer(9)"),
        "the capture failure must name both constructions: {message}"
    );
}
