# Annotations

Annotations link source code symbols to spec operation names. Each annotation
contributes a piece to an operation — the harness collects all annotations
sharing the same operation name.

See `rust.md` or `csharp.md` for language-specific syntax.

## Annotation types

| Annotation | Placed on | Purpose |
|------------|-----------|---------|
| **Operation** | Entry point method | Marks the function the spec tests. Kind is required. |
| **Setup** | Free function or constructor | Constructs objects or configures environment. Must not take `self`/`this`. Name is required. |
| **Checkpoint** (attribute) | Method | Every call to this method is recorded as an intermediate value. |
| **Checkpoint** (inline) | Any expression | Records a specific expression value at a point in execution. |
| **Capture** | Struct/class or field/property | Marks fields for output capture. On a struct: captures all public fields. On a field: captures that field only. |
| **Mock** | Method calling external service | Makes function mockable in test builds. Name is required. |

## Rules

- **Operation** requires both the operation name and `kind`
- **Setup** and **Mock** require both the operation name and a `name`
- **Setup** must NOT take `self`/`this`
- **Capture** can be placed on a struct (all public fields) or on individual fields
- Only one **Operation** per operation name per project
- **Setup** and **Mock** names must be unique per operation

## How annotations compose

All annotations sharing the same operation name are collected into one operation.
The operation's `kind` determines which annotations are valid — see `kinds.md`.

```
Operation("canvas", kind=StateMachine)  ─┐
Setup("canvas", name="default")         ─┤  → one operation: "canvas"
Capture("canvas") on struct fields       ─┤
Mock("canvas", name="renderer")         ─┘
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
