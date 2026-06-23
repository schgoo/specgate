# Annotations

Annotations link source code symbols to spec operation / setup / event /
mock names. They serve two purposes:

1. **Discovery** — the harness finds what to call.
2. **Instrumentation** — the runtime emits trace events during execution.

## Trace model

All traces use exactly two event types:

- `Event { name, value }` — any value observation (field mutation,
  return value, mock interaction, inline checkpoint, setup parameter).
- `Run { operation }` — marks where an operation invocation begins.

Position in the sequence determines before/after semantics. Events
before a `Run` for op X are "pre-X"; events after are "post-X". The
spec author rarely thinks in these terms — they just list events in the
order they care about.

`value` is structured. The runtime preserves scalars and collections as a
`Value` rather than flattening everything to strings, so specs can match on
lists, maps, sets, numeric ranges, and nested shapes.

## The annotation surface

| Annotation | Placed on | Purpose | Trace emitted |
|------------|-----------|---------|---------------|
| `#[spec_operation("name")]` | Free function or method | Marks the operation a case invokes by `operation:`/`steps[].operation:`. | `Run { operation: name }` at the entry point, plus per-parameter `Event { "<name>.<param>", value }` and auto-generated result events addressable from specs as `$result` / `$outcome` / `$error`. |
| `#[spec_setup("operation"[, fills = "param"])]` | Free function (no `self`) | Links a constructor to the **operation** it prepares. Setups are invisible to the spec. The return value fills the operation's method receiver or a parameter, matched by type; `fills` pins the target param when several share that type or several setups produce it (stackable). | None — setups emit no events of their own; the constructed value's `#[spec_event]` fields are emitted before the operation runs. |
| `#[derive(SpecEvent)]` | Struct or enum | Enables generated event emission for returned / observed values. | Structs emit named field values; enums emit the variant name plus named-field payload events. |
| `#[spec_event]` | Struct field (with `#[derive(SpecEvent)]` on the struct) | Every write to the field emits an event. | `Event { name: "<field>", value: new_value }` on each mutation. Setup-filled parameters prefix with the parameter role (`source.balance`). |
| `spec_trace!("name", expr)` | Inline expression | Records the value of `expr` at this point in execution. | `Event { name, value }` using the structured `Value` representation. |
| `#[spec_input("name")]` | Function parameter (of a `#[spec_operation]` / `#[spec_setup]` fn) | Gives the parameter a language-neutral spec name, decoupled from the code parameter name. | The case binds inputs and `op.<name>` events use this name instead of the code identifier. |
| `#[spec_mock("name")]` | Local binding around a method call | Intercepts the call and returns the case-supplied response. | `Event { "<name>.request", input }` then `Event { "<name>.response", mocked_response }`. |

**No `kind` parameter.** Every `#[spec_operation("…")]` in the fixtures
is name-only. The shape of the operation is expressed entirely by the
contents of the spec's `expected:` list.

**No `spec_capture` or `spec_checkpoint!()` annotations.** Field capture is
`#[derive(SpecEvent)]` + `#[spec_event]` on fields; inline capture is
`spec_trace!()`. Those two cover every observation pattern.

## `#[derive(SpecEvent)]`

`SpecEvent` derive is how structured values become traceable without writing
manual serialization code.

### Structs

For structs, derive enables `#[spec_event]` fields and preserves collection
shape:

```rust
#[derive(SpecEvent)]
pub struct EntityType {
    #[spec_event(name = "entity_name")]
    pub name: String,
    #[spec_event(name = "key_properties")]
    pub key_properties: Vec<String>,
    #[spec_event(name = "structural_properties")]
    pub structural_properties: Vec<String>,
}
```

This emits structured `List` values, which specs can match directly or with
operators like `$size` and `$contains`. Canonical fixture:
`test/rust/crates/specgate-fixtures/src/structured_output.rs`.

### Enums

For enums, derive supports:

- unit variants (`Point`)
- named-field variants (`Circle { radius }`, `Rectangle { width, height }`)

Example:

```rust
#[derive(SpecEvent)]
pub enum Shape {
    Circle { radius: f64 },
    Rectangle { width: f64, height: f64 },
    Point,
}
```

The trace emits the variant tag on the base name plus named-field payloads
on subpaths:

- `shape: "Point"`
- `shape: "Circle"` and `shape.radius: "5"`
- `shape: "Rectangle"`, `shape.width: "3"`, `shape.height: "4"`

Canonical fixture: `test/rust/crates/specgate-fixtures/src/enum_event.rs`.

## `spec_trace!()`

Use `spec_trace!()` for inline checkpoints when a value is not naturally
exposed through a captured field or return value.

```rust
#[spec_operation("process")]
pub fn process(data: &str) -> String {
    let upper = data.to_uppercase();
    spec_trace!("after_upper", &upper);
    upper.trim().to_string()
}
```

This is the canonical way to emit intermediate structured observations.
Canonical fixture:
`test/rust/crates/specgate-fixtures/src/checkpoint_inline.rs`.

## Rules

- `#[spec_setup]` functions must **not** take `self` / `this`.
- `#[spec_input("name")]` on a parameter of a `#[spec_operation]` / `#[spec_setup]`
  function renames that input to a language-neutral spec name. The spec and case
  use `name`; the code keeps its own parameter name. Without it, the spec name
  defaults to the code parameter name.
- `#[spec_setup("operation")]` takes the **operation** name it prepares, not
  its own name. Setups never appear in a spec.
- A setup's return value fills the operation's method receiver or a parameter
  **by type**. When several parameters share that type, or several setups
  produce it, each setup must pin its target with `fills = "<param>"`.
- Several `#[spec_setup(..., fills = ...)]` attributes may be stacked on one
  function to build several same-typed parameters.
- An operation name may have at most one `#[spec_operation]` in a given
  source file. Naming-lookup is scoped per source file (so two fixtures
  can share an operation name without colliding).
- A `#[spec_mock]` name must be unique within an operation.
- `#[spec_event]` on a field captures **every** mutation, including the
  initial value set by the setup function. This is why so many fixtures
  show a leading `count: "0"` (or similar) before the first `$run:`.
- Use `#[spec_event(name = "...")]` when the emitted field name in the
  spec should differ from the source field name.

## How annotations compose

A single operation typically uses several annotations across one source
file. They are joined at runtime by name and source-file scope.

```
#[spec_setup("increment")]  on make_counter        ─┐
#[derive(SpecEvent)] on Counter                     │
  #[spec_event] on Counter.count                   ─┼─► one operation, "increment"
#[spec_operation("increment")] on Counter::incr    ─┘
```

See `test/rust/crates/specgate-fixtures/src/statemachine_counter.rs`
for the full example.

## Reference fixtures

| Pattern | Source file |
|---------|-------------|
| `#[spec_operation]` only (no setup) | `stateless_add.rs` |
| `#[spec_setup]` + `#[spec_event]` + method op | `statemachine_counter.rs` |
| Multiple `#[spec_event]` fields | `multi_field_capture.rs` |
| `spec_trace!()` inline checkpoint | `checkpoint_inline.rs` |
| Structured value emission (`Vec`, `BTreeMap`) | `structured_output.rs`, `operators.rs` |
| Enum `#[derive(SpecEvent)]` | `enum_event.rs` |
| `#[spec_mock]` | `mock_field.rs`, `mock_multi_response.rs` |
| Setup with a construction input | `setup_with_params.rs` |
| Multiple setups, same type (`fills`) | `multi_setup.rs` |
| One setup filling several params (stacked `fills`) | `shared_setup.rs` |
| Side-effect / simple-output setup | `side_effect_setup.rs`, `simple_output_setup.rs` |
| Language-neutral input names (`#[spec_input]`) | `named_inputs.rs` |
| `Result` return | `result_ok.rs`, `result_err.rs` |
| `Option` return | `option_some.rs` |
| Panic / unrecoverable | `unrecoverable.rs` |
| Void operation | `void_operation.rs` |
| Nested operation calls | `nested_operations.rs` |

## Zero-cost in production

Annotations are expected to be **no-ops** when the SpecGate feature
flag is disabled (Rust) or the `SPECGATE` define is absent (C#). With
the flag off the macros expand to nothing — no trace buffer, no event
records, no cost.

| Build mode | Behaviour |
|------------|-----------|
| Tests with `specgate` feature on | Annotations active, events captured |
| Release build, feature off | Annotations are no-op; zero overhead |
