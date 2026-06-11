# Annotations

Annotations link source code symbols to spec operation names. Each annotation
contributes a piece to an operation — the harness collects all annotations
sharing the same operation name.

See `rust.md` or `csharp.md` for language-specific syntax.

## Annotation types

| Annotation | Form | Placed on | Purpose |
|------------|------|-----------|---------|
| **Operation** | attribute | Entry point method | Marks the function the spec tests. Kind is required. |
| **Setup** | attribute | Free function or constructor | Constructs objects or configures environment. Must not take `self`/`this`. Name is required. |
| **Checkpoint** | attribute | Method | Every call to this method is recorded as an intermediate value. |
| **Checkpoint** | inline | Any expression | Records this specific expression value at this point in execution. Works on third-party types. |
| **Capture** | attribute | Struct or field | Marks fields for output capture. On a struct: captures all public fields. On a field: captures that field only. |
| **Mock** | attribute | Method calling external service | Makes function mockable in test builds. Name is required. |

## Rules

- **Operation** requires both the operation name and `kind`
- **Setup** and **Mock** require both the operation name and a `name`
- **Setup** must NOT take `self`/`this` — it's a free function, static method, or associated function
- **Capture** can be placed on a struct (all public fields) or on individual fields
- **Checkpoint** attribute goes on a method definition; inline form wraps any expression
- Only one **Operation** per operation name per project (duplicates fail validation)
- **Setup** names must be unique per operation within a project
- **Mock** names must be unique per operation within a project
- Async and generic functions work normally with all annotations

## How annotations compose

All annotations sharing the same operation name are collected into one operation.
The operation's `kind` (from the Operation annotation) determines which other
annotations are valid — see `kinds.md` for the full matrix.

```
Operation("calc", kind=StateMachine)  ─┐
Setup("calc", name="default")         ─┤  → one operation: "calc"
Capture("calc") on struct fields       ─┤
Checkpoint("calc") on methods          ─┤
Mock("calc", name="backend")          ─┘
```

## Output capture

The harness needs to capture operation outputs as JSON for comparison against
spec expectations. Two mechanisms are available:

1. **Capture (field-level):** place `spec_capture` on the struct or on
   individual fields. The macro reads annotated fields and serializes them to
   JSON automatically. No `Serialize` derive is needed on the user's types.
   On a struct, all public fields are captured. On individual fields, only
   annotated fields are captured.
2. **Checkpoint (expression-level):** use the inline `spec_checkpoint!()` form
   to capture any expression, including calls to third-party types. The value
   is recorded and returned so execution continues normally.

For StateMachine operations, captured fields are recorded **before and after**
the operation call, producing state transition data. For all other kinds,
fields are captured **after** the call only.

## Zero-cost in production

All annotations are **no-op in release builds** by default. The capture,
checkpoint, and mock instrumentation is compiled out unless explicitly enabled.

| Build mode | Behavior |
|------------|----------|
| `debug` (default) | Annotations are active — values are captured, mocks can be injected |
| `release` | Annotations are no-op — zero runtime overhead, zero binary size impact |
| Feature flag off | Annotations are no-op regardless of build mode |

This is controlled by a Cargo feature (Rust) or build configuration (C#):

- **Rust:** the annotation crate exposes a `specgate` feature. When disabled,
  all attribute macros expand to nothing and all inline macros evaluate to the
  inner expression unchanged. Release builds disable the feature by default.
- **C#:** the annotation package uses `[Conditional("SPECGATE")]` attributes.
  The `SPECGATE` define is set in Debug builds and absent in Release builds.

Users can force annotations on in release builds (e.g. for integration testing
in a staging environment) by explicitly enabling the feature/define.
