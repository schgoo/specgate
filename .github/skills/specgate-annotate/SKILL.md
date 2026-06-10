---
name: specgate-annotate
description: >
  Annotates C# code with SpecGate attributes for spec extraction, then invokes
  /specgate-review to validate the annotations and iterates until the review passes.
  Use when the user asks to annotate code, add spec annotations, or mark up a
  method/class for SpecGate. Trigger phrases: "annotate", "add spec annotations",
  "mark up for specgate", "specgate annotate", "/specgate-annotate".
---

# SpecGate C# Annotation Skill

You are annotating C# code with SpecGate attributes. Your job is to add `[Spec*]`
attributes to existing code so that the SpecGate extractor can later generate a
formal spec from it.

**Do NOT modify any logic, signatures, or behavior — only add attributes.**

**Edit files in place** — use file editing tools to add attributes directly to the
source files. Do not just output annotated code; make the actual changes. Add
`using SpecGate;` to the file's imports if not already present.

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
5. **Constructor inputs**: all constructor parameters become operation inputs.
6. **Properties of `this` used by the method**: any property of `this` that the method reads is a candidate for `[SpecInput]`, `[SpecEnvironment]`, or `[SpecDependency]`.
7. **Dependencies**: external HTTP calls, database calls, non-deterministic methods (clock, random) get `[SpecDependency]`.
8. **Environments**: config, settings, ambient state that is read but not injected per-call.
9. **Checkpoints**: internal methods whose return values are observable intermediates in a Sequence.
10. **Do NOT annotate**: `[CallerFilePath]`, `[CallerMemberName]`, `[CallerLineNumber]` parameters — these are compiler-injected, not operation inputs.
11. **Do NOT annotate**: logging, telemetry, or metrics calls — these are side effects, not spec-relevant.
12. **Visibility**: do not change access modifiers. Just add attributes.

## Role validity per kind

| Role | Pure | StateMachine | Sequence | ErrorMap | Structural |
|------|------|-------------|----------|----------|------------|
| Input | ✅ | ✅ | ✅ | ✅ | ❌ |
| State | ❌ | ✅ | ❌ | ❌ | ❌ |
| Checkpoint | ❌ | ❌ | ✅ | ❌ | ❌ |
| Environment | ✅ | ✅ | ✅ | ✅ | ❌ |
| Dependency | ✅ | ✅ | ✅ | ✅ | ❌ |

## Required roles per kind

| Kind | Must have |
|------|----------|
| StateMachine | ≥1 State |
| Sequence | ≥1 Checkpoint |
| ErrorMap | ≥1 error-returning path |
| Pure | Nothing beyond the entry point |
| Structural | Nothing (no runtime) |

## Process

1. **Read the target code** — understand the method, its class, constructor, properties, base class members, and any internal methods it calls.
2. **Choose the kind** — based on what the operation does.
3. **Identify the operation name** — camelCase, descriptive.
4. **Edit the source files in place** — add attributes directly to the code:
   a. Add `using SpecGate;` to imports if not present.
   b. Add `[SpecOperation]` to the entry point method.
   c. Add `[SpecInput]` to the constructor and input properties.
   d. Add `[SpecEnvironment]` to ambient/config state properties.
   e. Add `[SpecDependency]` to external service properties or methods.
   f. Add `[SpecCheckpoint]` to internal methods with observable intermediates (Sequence only).
   g. Add `[SpecState]` to state snapshot fields/properties (StateMachine only).
5. **Check for unconstructable types** — for every type used as an input, environment,
   or dependency response, navigate to the type's source and check whether it has a
   public constructor with decomposable parameters. If it doesn't, flag it and add a
   `[SpecGenerator]` annotation on a factory method, or report that one is needed.
6. **Invoke `/specgate-review`** on the annotated files to validate.
7. **Iterate** — fix any issues the review surfaces by editing the files again, re-review until it passes.
8. **Present a summary** of the changes made.

## Output format

After the review passes, present:
1. A list of files modified and what annotations were added.
2. A brief summary of annotation decisions:
   - Why you chose the kind
   - What each annotation role captures
   - Any types that may be unconstructable
   - Any ambiguities and how they were resolved
