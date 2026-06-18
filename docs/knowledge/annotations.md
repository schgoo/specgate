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

## The five annotations

| Annotation | Placed on | Purpose | Trace emitted |
|------------|-----------|---------|---------------|
| `#[spec_operation("name")]` | Free function or method | Marks the operation a case invokes by `operation:`/`steps[].operation:`. | `Run { operation: name }` at the entry point, plus per-parameter `Event { "<name>.<param>", value }` and `Event { "<name>.result", value }` for the return value (or `<name>.outcome` + `<name>.error` for `Result`). |
| `#[spec_setup("name")]` | Free function (no `self`) | Names a factory a case invokes by `setup:`. | `Event { "<name>.<param>", value }` per parameter. |
| `#[spec_event]` | Struct field (with `#[derive(SpecEvent)]` on the struct) | Every write to the field emits an event. | `Event { name: "<field>", value: new_value }` on each mutation. Multi-setup cases prefix with the alias (`source.balance`). |
| `spec_event_record!("name", expr)` | Inline expression | Records the value of `expr` at this point in execution. | `Event { name, value: format!("{}", expr) }`. |
| `#[spec_mock("name")]` | Local binding around a method call | Intercepts the call and returns the case-supplied response. | `Event { "<name>.request", input }` then `Event { "<name>.response", mocked_response }`. |

**No `kind` parameter.** Every `#[spec_operation("…")]` in the fixtures
is name-only. The shape of the operation is expressed entirely by the
contents of the spec's `expected:` list.

**No `spec_capture` or `spec_checkpoint!()` annotations.** Field capture
is `#[derive(SpecEvent)]` + `#[spec_event]` on fields; inline capture is
`spec_event_record!()`. Those two cover every observation pattern.

## Rules

- `#[spec_setup]` functions must **not** take `self` / `this`.
- An operation name may have at most one `#[spec_operation]` in a given
  source file. Naming-lookup is scoped per source file (so two fixtures
  can share an operation name without colliding).
- A `#[spec_mock]` name must be unique within an operation.
- `#[spec_event]` on a field captures **every** mutation, including the
  initial value set by the setup function. This is why so many fixtures
  show a leading `count: "0"` (or similar) before the first `run:`.

## How annotations compose

A single operation typically uses several annotations across one source
file. They are joined at runtime by name and source-file scope.

```
#[spec_setup("make_counter")]                      ─┐
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
| `spec_event!()` inline | `checkpoint_inline.rs` |
| `#[spec_mock]` | `mock_field.rs`, `mock_multi_response.rs` |
| Setup with parameters | `setup_with_params.rs` |
| Multiple setups | `multi_setup.rs` |
| `Result` return | `result_ok.rs` (Ok and Err paths share this file) |
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
