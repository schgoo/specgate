use serde_yaml::Value;
use specgate_types::{BindingDecl, BindingEntry, SpecDocument};

fn parse_spec(input: &str) -> Result<SpecDocument, serde_yaml::Error> {
    serde_yaml::from_str(input)
}

fn expect_valid(input: &str) -> SpecDocument {
    parse_spec(input).expect("spec should parse")
}

fn expect_invalid(input: &str, reason: &str) {
    let error = parse_spec(input).expect_err("spec should be invalid");
    assert_eq!(error.to_string(), reason);
}

#[test]
fn single_operation_minimal() {
    let spec = expect_valid(
        r"
name: test.component
binding:
  name: rust
  target: test
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: basic
    desc: Basic test
    expected:
      outcome: Ok
      result: hello
",
    );

    assert_eq!(spec.binding_name().as_deref(), Some("rust"));
    assert_eq!(spec.target().as_deref(), Some("test"));
}

#[test]
fn single_operation_with_inputs_and_types() {
    let spec = expect_valid(
        r"
name: test.component
binding:
  name: rust
  target: test
inputs:
  shape:
    type: Shape
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }
outcome:
  oneof: [Ok, Error]
outputs:
  when Ok:
    area: float
  when Error:
    message: string
cases:
  - name: circle
    desc: Circle area
    inputs:
      shape:
        Circle: { radius: 5.0 }
    expected:
      outcome: Ok
      area: 78.54
",
    );

    assert!(spec.inputs.contains_key("shape"));
    assert!(spec.types.contains_key("Shape"));
}

#[test]
fn state_machine_minimal() {
    let spec = expect_valid(
        r"
name: test.machine
binding:
  name: rust
  target: test
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs:
      amount:
        type: int
cases:
  - name: inc_once
    desc: Increment once
    steps:
      - operation: increment
        inputs: { amount: 1 }
        assert_state: { count: 1 }
",
    );

    assert_eq!(spec.outcome, Value::Null);
    assert!(spec.state.contains_key("count"));
    assert!(spec.operations.contains_key("increment"));
}

#[test]
fn state_machine_with_invariants() {
    let spec = expect_valid(
        r#"
name: test.machine
binding:
  name: rust
  target: test
state:
  items: Set<string>
init:
  items: []
operations:
  add_item:
    inputs:
      name:
        type: string
invariants:
  never_empty_after_add: "items.size() >= 1"
cases:
  - name: add_one
    desc: Adding an item
    steps:
      - operation: add_item
        inputs: { name: foo }
        assert_state:
          items: [foo]
"#,
    );

    assert_eq!(
        spec.invariants.get("never_empty_after_add").map(String::as_str),
        Some("items.size() >= 1")
    );
}

#[test]
fn reject_inputs_and_operations() {
    expect_invalid(
        r"
name: test.invalid
binding:
  name: rust
  target: test
inputs:
  x:
    type: int
operations:
  foo:
    inputs: { y: int }
outcome: Ok
outputs:
  when Ok:
    result: int
cases:
  - name: bad
    desc: Invalid
    expected:
      outcome: Ok
      result: 1
",
        "inputs and operations are mutually exclusive",
    );
}

#[test]
fn reject_steps_without_operations() {
    expect_invalid(
        r"
name: test.invalid
binding:
  name: rust
  target: test
outcome: Ok
outputs:
  when Ok:
    result: int
cases:
  - name: bad
    desc: Invalid
    steps:
      - operation: foo
        inputs: { x: 1 }
",
        "steps require operations section",
    );
}

#[test]
fn reject_flat_inputs_in_state_machine_case() {
    expect_invalid(
        r"
name: test.invalid
binding:
  name: rust
  target: test
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs: { amount: int }
cases:
  - name: bad
    desc: Invalid
    inputs:
      amount: 1
    expected:
      outcome: Ok
",
        "state machine cases must use steps",
    );
}

#[test]
fn depends_on_list() {
    let spec = expect_valid(
        r"
name: test.consumer
binding:
  name: rust
  target: test
depends_on:
  - core.spec_document
  - core.types
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: basic
    desc: Basic
    expected:
      outcome: Ok
      result: hello
",
    );

    assert_eq!(spec.depends_on, vec!["core.spec_document", "core.types"]);
}

#[test]
fn reject_missing_name() {
    expect_invalid(
        r"
binding:
  name: rust
  target: test
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: basic
    desc: Basic
    expected:
      outcome: Ok
      result: hello
",
        "missing required field: name",
    );
}

#[test]
fn reject_missing_cases() {
    expect_invalid(
        r"
name: test.component
binding:
  name: rust
  target: test
outcome: Ok
outputs:
  when Ok:
    result: string
",
        "missing required field: cases",
    );
}

#[test]
fn reject_state_without_init() {
    expect_invalid(
        r"
name: test.machine
binding:
  name: rust
  target: test
state:
  count: int
operations:
  increment:
    inputs: { amount: int }
cases:
  - name: inc
    desc: Increment
    steps:
      - operation: increment
        inputs: { amount: 1 }
",
        "state requires init",
    );
}

#[test]
fn multi_binding_list() {
    let spec = expect_valid(
        r"
name: core.shared_types
binding:
  - name: rust
    target: test
  - name: csharp
    target: test
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: basic
    desc: Basic
    expected:
      outcome: Ok
      result: hello
",
    );

    assert_eq!(spec.binding_name().as_deref(), Some("rust"));
    assert_eq!(spec.target().as_deref(), Some("test"));
    match spec.binding {
        Some(BindingDecl::Multiple(bindings)) => assert_eq!(bindings.len(), 2),
        other => panic!("expected multiple bindings, got {other:?}"),
    }
}

#[test]
fn no_binding() {
    let spec = expect_valid(
        r"
name: test.component
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: basic
    desc: Basic
    expected:
      outcome: Ok
      result: hello
",
    );

    assert!(spec.binding.is_none());
    assert_eq!(spec.binding_name(), None);
    assert_eq!(spec.target(), None);
}

#[test]
fn types_with_causes() {
    let spec = expect_valid(
        r#"
name: test.component
binding:
  name: rust
  target: test
types:
  MyError:
    causes:
      NotFound: { id: string }
      PermissionDenied: { user: string, resource: string }
inputs:
  id:
    type: string
outcome:
  oneof: [Ok, Error]
outputs:
  when Ok:
    result: string
  when Error:
    error: MyError
cases:
  - name: not_found
    desc: Returns not found
    inputs:
      id: "missing"
    expected:
      outcome: Error
"#,
    );

    assert!(spec.types.contains_key("MyError"));
}

#[test]
fn types_with_fields() {
    let spec = expect_valid(
        r#"
name: test.component
binding:
  name: rust
  target: test
types:
  Config:
    fields:
      timeout_ms: int
      retries: int
      endpoint: string
inputs:
  config:
    type: Config
outcome: Ok
outputs:
  when Ok:
    result: string
cases:
  - name: with_config
    desc: Accepts config
    inputs:
      config: { timeout_ms: 5000, retries: 3, endpoint: "http://localhost" }
    expected:
      outcome: Ok
      result: ok
"#,
    );

    assert!(spec.types.contains_key("Config"));
}

#[test]
fn reject_case_with_inputs_and_steps() {
    expect_invalid(
        r"
name: test.machine
binding:
  name: rust
  target: test
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs: { amount: int }
cases:
  - name: bad
    desc: Has both inputs and steps
    inputs:
      amount: 1
    steps:
      - operation: increment
        inputs: { amount: 1 }
",
        "case cannot have both inputs and steps",
    );
}

#[test]
fn state_machine_case_postconditions() {
    let spec = expect_valid(
        r#"
name: test.machine
binding:
  name: rust
  target: test
state:
  count: int
init:
  count: 0
operations:
  increment:
    inputs: { amount: int }
cases:
  - name: cleanup_verified
    desc: Cleanup verified after run
    steps:
      - operation: increment
        inputs: { amount: 1 }
        assert_state: { count: 1 }
    postconditions:
      - target: assert-file-absent
        inputs:
          path: "{generated_test_path}"
        desc: generated file removed
"#,
    );

    let postconditions = spec.cases[0].postconditions.as_ref().expect("postconditions should deserialize");
    assert_eq!(postconditions.len(), 1);
    assert_eq!(postconditions[0].target, "assert-file-absent");
    assert_eq!(
        postconditions[0].inputs.get("path").map(String::as_str),
        Some("{generated_test_path}")
    );
    assert_eq!(postconditions[0].desc.as_deref(), Some("generated file removed"));
}

#[test]
fn reject_legacy_binding_without_target() {
    expect_invalid(
        r"
name: legacy
binding: rust
outcome: Ok
cases: []
",
        "missing field `target`",
    );
}

#[test]
fn binding_decl_first_returns_first_multiple_binding() {
    let binding = BindingDecl::Multiple(vec![
        BindingEntry {
            name: "rust".to_string(),
            target: "test".to_string(),
        },
        BindingEntry {
            name: "csharp".to_string(),
            target: "test".to_string(),
        },
    ]);

    let first = binding.first().expect("multiple binding should have first entry");
    assert_eq!(first.name, "rust");
    assert_eq!(first.target, "test");
}
