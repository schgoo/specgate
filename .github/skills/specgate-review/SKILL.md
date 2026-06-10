---
name: specgate-review
description: >
  Reviews SpecGate C# annotations for correctness, completeness, and consistency.
  Use when the user asks to review annotations, validate spec markup, or check
  specgate attributes. Trigger phrases: "review annotations", "check annotations",
  "validate specgate", "specgate review", "/specgate-review".
---

# SpecGate Annotation Review Skill

You are reviewing C# code that has been annotated with SpecGate `[Spec*]` attributes.
Your job is to validate that the annotations are correct, complete, and consistent.

**Do NOT modify the code. Only report findings.**

## What to check

### 1. Kind correctness

Is the chosen `SpecKind` appropriate for this operation?

- `Pure`: Does the method actually produce the same output for the same inputs? Are there hidden side effects?
- `Sequence`: Are there observable intermediate steps? If not, should this be `Pure`?
- `StateMachine`: Does it actually transition between states?
- `ErrorMap`: Does it actually map error conditions?

### 2. Role validity

Check the role validity matrix. Flag any invalid combinations:

| Role | Pure | StateMachine | Sequence | ErrorMap | Structural |
|------|------|-------------|----------|----------|------------|
| Input | ✅ | ✅ | ✅ | ✅ | ❌ |
| State | ❌ | ✅ | ❌ | ❌ | ❌ |
| Checkpoint | ❌ | ❌ | ✅ | ❌ | ❌ |
| Environment | ✅ | ✅ | ✅ | ✅ | ❌ |
| Dependency | ✅ | ✅ | ✅ | ✅ | ❌ |

### 3. Required roles

| Kind | Must have |
|------|----------|
| StateMachine | ≥1 State |
| Sequence | ≥1 Checkpoint |
| ErrorMap | ≥1 error-returning path |
| Pure | Nothing beyond the entry point |
| Structural | Nothing (no runtime) |

### 4. Completeness

- **Missing inputs**: Does the method use properties or constructor parameters that are not annotated?
- **Missing dependencies**: Does the method call external services, HTTP clients, databases, or non-deterministic methods that are not annotated?
- **Missing environments**: Does the method read configuration or ambient state that is not annotated?
- **Missing checkpoints**: For Sequence operations, are there internal methods producing observable values that are not annotated?

### 5. Over-annotation

- **Annotated telemetry/logging**: Logging, metrics, and telemetry should NOT be annotated.
- **Annotated compiler-injected parameters**: `[CallerFilePath]`, `[CallerMemberName]`, `[CallerLineNumber]` should NOT be annotated.
- **Redundant annotations**: Same role on the same element twice.
- **Implementation-coupled checkpoints**: Checkpoints that capture internal implementation details rather than spec-meaningful intermediates.

### 6. Naming consistency

- Is the operation name camelCase and descriptive?
- Do all annotations for the same operation use the exact same name string?
- Are dependency `Dep` values descriptive and consistent?

### 7. Unconstructable types

For every type used as an input, environment, or dependency response:
- **Navigate to the type's source code** and inspect its constructors.
- A type is unconstructable if it has no public constructor whose parameters are
  themselves decomposable (primitives, strings, or other constructable types).
- If unconstructable, check whether a `[SpecGenerator]` already exists for it.
- If not, report it as a warning with the type name and what's missing.

### 8. Behavioral concerns

- Does the method have non-deterministic behavior that is not captured by a `[SpecDependency]`?
- Are there ambient state reads that are not captured by a `[SpecEnvironment]`?
- Could the outcome be ambiguous? (e.g., method returns null — is that `NotFound` or `Error`?)

## Output format

Report findings in three categories:

### ❌ Errors (must fix)
Issues that would make the spec incorrect or invalid. Examples:
- Invalid role+kind combination
- Missing required roles
- Wrong operation name on one annotation

### ⚠️ Warnings (should fix)
Issues that would make the spec incomplete or misleading. Examples:
- Missing dependency annotation
- Annotated telemetry call
- Potentially unconstructable type

### ℹ️ Notes (consider)
Suggestions and observations. Examples:
- Alternative kind that might fit better
- Ambiguous outcome that should be clarified
- Property that could be Input or Environment depending on intent

For each finding, state:
1. **What**: the specific issue
2. **Where**: the code location
3. **Why**: why it matters for spec correctness
4. **Fix**: what to do about it

End with a **verdict**: `PASS` (no errors), `PASS WITH WARNINGS`, or `FAIL` (has errors).
