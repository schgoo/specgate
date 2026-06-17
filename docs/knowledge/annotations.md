# Annotations

Annotations link source code symbols to spec operation names. They serve two
purposes: discovery (the harness finds what to call) and instrumentation (the
runtime emits trace events during execution).

## Trace model

All traces use two event types:
- `Event { name, value }` — any observation (state snapshot, return value, checkpoint, mock interaction)
- `Run { operation }` — marks when an operation executes

Position in the sequence determines before/after semantics. Events before a
`Run` are pre-state; events after are post-state.

## Annotation types

| Annotation | Placed on | Purpose | Trace emitted |
|------------|-----------|---------|---------------|
| **`#[spec_operation]`** | Entry point method | Marks the function the spec tests. Kind required. | `Run { operation }` |
| **`#[spec_setup]`** | Free function or constructor | Constructs objects. Must not take `self`. | `Event { name, value }` for each argument |
| **`#[spec_event]`** | Struct field or method | On a field: emits `Event` on every mutation and at operation boundaries. On a method: captures return value. | `Event { name, value }` |
| **`spec_event!()`** | Inline expression | Records a value at a specific point in execution. | `Event { name, value }` |
| **`#[spec_mock]`** | Method calling external service | Makes function mockable. Records call input/output. | `Event { name: "mock.input/output", value }` |

## Rules

- **Operation** requires both the operation name and `kind`
- **Setup** must NOT take `self`/`this`
- **`spec_event`** on a field emits a trace on every mutation and at operation boundaries
- **`spec_event`** on a method captures the return value after the operation
- Only one **Operation** per operation name per project
- **Mock** names must be unique per operation

## How annotations compose

All annotations sharing the same operation name are collected into one operation.

```
#[spec_operation("canvas", kind=StateMachine)] ─┐
#[spec_setup("canvas")]                        ─┤  → one operation: "canvas"
#[spec_event] on struct fields                 ─┤
#[spec_mock("canvas", name="renderer")]        ─┘
```

## Output capture

Two mechanisms capture operation outputs for comparison:

1. **Capture (field-level):** `spec_capture` on a struct or individual fields.
   On a struct, all public fields are captured. On individual fields, only
   annotated fields are captured.
2. **Checkpoint (expression-level):** inline `spec_checkpoint!()` captures any
   expression, including calls to third-party types.

For StateMachine operations, captured fields are recorded **before and after**
the operation call. For all other kinds, fields are captured **after** only.

## Zero-cost in production

All annotations are **no-op in release builds** by default. The capture,
checkpoint, and mock instrumentation is compiled out unless explicitly enabled.

| Build mode | Behavior |
|------------|----------|
| `debug` (default) | Annotations are active — values are captured, mocks can be injected |
| `release` | Annotations are no-op — zero runtime overhead, zero binary size impact |
| Feature flag off | Annotations are no-op regardless of build mode |

- **Rust:** feature flag controls activation. Disabled = macros expand to nothing.
  `spec_checkpoint!(expr)` evaluates to just `expr`.
- **C#:** `[Conditional("SPECGATE")]` attributes. Absent define = no-op.
  `SpecCheckpoint.Capture()` compiles to a pass-through.

Users can force annotations on in release builds (e.g. for integration testing
in staging) by explicitly enabling the feature/define.
