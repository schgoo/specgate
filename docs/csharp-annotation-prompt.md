# SpecGate C# annotation prompt

You are annotating C# code with SpecGate attributes. Your job is to add `[Spec*]`
attributes to existing code so that the SpecGate extractor can later generate a
formal spec from it. Do NOT modify any logic, signatures, or behavior — only add
attributes.

## Available attributes

All attributes are in the `SpecGate` namespace.

| Attribute | Placed on | Purpose |
|-----------|-----------|---------|
| `[SpecOperation("name", SpecKind.X)]` | Entry point method | Identifies the operation and its kind |
| `[SpecInput("name")]` | Constructor, property, setter | Marks a parameter or value as an operation input |
| `[SpecCheckpoint("name")]` | Internal method | Observable intermediate value |
| `[SpecState("name")]` | Field or property | State snapshot (StateMachine only) |
| `[SpecEnvironment("name")]` | Property or method | Ambient state the operation reads |
| `[SpecDependency("name", Dep = "x")]` | Property or method | External service or non-deterministic call |

## Kinds

| Kind | When to use |
|------|------------|
| `Pure` | Same inputs → same output, no side effects |
| `Sequence` | Ordered steps with observable intermediates (checkpoints) |
| `StateMachine` | Transitions between states with invariants |
| `ErrorMap` | Maps error conditions to specific error types/codes |
| `Structural` | Static analysis only — no runtime annotations |

## Rules

1. **Operation name**: use camelCase, descriptive of what the operation does (e.g., `"findByTypeName"`).
2. **All annotations for one operation share the same name string.**
3. **`Task<T>` unwraps**: the spec sees `T`, not `Task<T>`.
4. **Generic methods**: annotate once. `T` is resolved later.
5. **Constructor inputs**: all constructor parameters become inputs.
6. **`this` properties used by the method**: any property of `this` that the method reads is a candidate for `[SpecInput]`, `[SpecEnvironment]`, or `[SpecDependency]`.
7. **Dependencies**: external HTTP calls, database calls, non-deterministic methods (clock, random) get `[SpecDependency]`.
8. **Environments**: config, settings, ambient state that is read but not injected per-call.
9. **Checkpoints**: internal methods whose return values are observable intermediates in a Sequence.
10. **Do NOT annotate**: `[CallerFilePath]`, `[CallerMemberName]`, `[CallerLineNumber]` parameters — these are compiler-injected, not operation inputs.
11. **Do NOT annotate**: logging, telemetry, or metrics calls — these are side effects, not spec-relevant.
12. **Visibility**: do not change access modifiers. Just add attributes.

## Output format

Return the annotated code with a brief explanation of each annotation decision:
- Why you chose the kind
- What each annotation role captures
- Any types that may be unconstructable (no public constructor or decomposable fields)
- Any ambiguities you encountered

## Target

{{TARGET_DESCRIPTION}}

```csharp
{{CODE}}
```
