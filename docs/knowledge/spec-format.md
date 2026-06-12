# Spec file format

Spec files are YAML, validated by `spec-schema.json`. One file per component or
logical group of operations.

**File convention**: `specs/<name>.spec.yaml`

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Dotted component name, e.g. `core.validate` |
| `binding` | no | Binding file identifier — resolves to `bindings/<binding>.yaml`. Optional for language-agnostic specs; can be specified at run time. |
| `target` | yes | Named target within the binding file |
| `inputs` | cond | Named inputs with type/source/desc (single-operation specs) |
| `types` | no | Named type definitions (oneof, causes, or record) |
| `outcome` | cond | Outcome variants or single type (single-operation specs) |
| `outputs` | cond | Per-outcome output fields (single-operation specs) |
| `state` | no | State variables and types (StateMachine specs) |
| `init` | no | Initial state values (required when `state` is present) |
| `operations` | no | Named operations with inputs/outcomes (StateMachine specs) |
| `invariants` | no | Approved invariants (proposed by `specgate propose-invariants`) |
| `cases` | yes | Test cases (≥1) |

A spec is either **single-operation** (has `inputs`/`outcome`/`outputs` at the top
level) or **multi-operation / state machine** (has `state`/`operations`). These are
mutually exclusive.

## Type declarations

Types can be inline or named. Use named types for variants (`oneof`), error
types (`causes`), or reuse.

```yaml
# Named type with variants (discriminated union)
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }

# Named error type with causes (error chain, not a discriminated union)
types:
  RunError:
    causes:
      SpecNotFound: { path: string }
      SpecInvalid: { detail: string }
      BindingNotFound: { binding: string }

# Named record type
types:
  Point:
    fields:
      x: float
      y: float
```

**`oneof` vs `causes`:** Use `oneof` for data variants the caller matches
exhaustively (e.g. Shape). Use `causes` for error types where each entry is a
possible failure cause — languages map this to error chains (Rust/ohno) or
exception hierarchies (C#), not discriminated unions.

Bare keys (without `fields:` wrapper) are also valid for record types:

```yaml
types:
  Point:
    x: float
    y: float
```

**Types are suggestions, not rules.** They describe what fields must be available,
not how the implementation structures data internally. A Rust enum, a struct, a
trait object — any representation works as long as test cases can construct inputs
and assert outputs using the declared fields.

## Inputs

```yaml
inputs:
  shape:
    type: Shape
  precision:
    type: int
    source: config
    desc: Decimal places for rounding
```

- `type` is required
- `source` is optional — tells the harness where the input comes from if not a function argument
- `desc` is optional human-readable description

## Outcomes and outputs

```yaml
outcome:
  oneof: [Ok, Error]

outputs:
  when Ok:
    result: float
  when Error:
    message: string
```

Outcome can also be a single string: `outcome: Ok`

### Outcome kinds

Outcomes fall into three categories based on how they map to language constructs:

| Outcome kind | Meaning | Rust | C# |
|---|---|---|---|
| Success (e.g. `Ok`, `Complete`) | Operation succeeded | Return value | Return value |
| Error (e.g. `Error`, `Invalid`) | Recoverable failure — caller can handle | `Result::Err(E)` | `Result<T>.Error` |
| `Unrecoverable` | Invariant violated — process must stop | `panic!()` | `throw Exception` |

**Error vs Unrecoverable:**

- **Error** — the caller can do something about it (retry, fallback, report to user).
  Model as a returned `Result` in all languages. The spec declares the error type and
  its fields. Generated tests check the returned error variant.
- **Unrecoverable** — the system is in a bad state and continuing would make things
  worse. The operation must abort. Model as panic (Rust) or throw (C#). The spec
  declares an expected message. Generated tests use `#[should_panic]` (Rust) or
  `Assert.Throws<>` (C#).

```yaml
# Example with all three outcome kinds
outcome:
  oneof: [Complete, Error, Unrecoverable]

outputs:
  when Complete:
    report: RunReport
  when Error:
    error: RunError
  when Unrecoverable:
    message: string

cases:
  - name: corrupted_registry
    desc: Aborts if annotation registry has impossible state
    inputs:
      spec_path: fixtures/corrupted_registry.spec.yaml
    expected:
      outcome: Unrecoverable
      message: "annotation registry is corrupted"
```

## Test cases

```yaml
cases:
  - name: circle_area
    desc: Computes area of a circle
    inputs:
      shape:
        Circle: { radius: 5.0 }
    expected:
      outcome: Ok
      result: 78.54
```

- `name` is unique, snake_case
- `desc` is required
- `inputs` is optional (if the component takes no inputs)
- `expected.outcome` is required, must match an outcome variant

## YAML quirks

- `List<T>` with angle brackets does not need quoting
- `[...]` is YAML flow sequence — quote if used as a string value
- `{ }` is YAML flow mapping — both inline and block indent are equivalent
- Add `# yaml-language-server: $schema=../spec-schema.json` at the top for editor support

## State machine specs

State machine specs describe components with multiple operations that share state.
They use `state`, `init`, `operations`, and optionally `invariants` instead of
top-level `inputs`/`outcome`/`outputs`.

```yaml
name: harness.core
binding: rust
target: test

state:
  backends: Set<string>

init:
  backends: [mock]

operations:
  register_backend:
    inputs: { name: string }
  run_spec:
    inputs: { spec_path: string }
    outcome:
      oneof: [Complete, Error]

invariants:
  mock_always_registered: "mock ∈ backends"
  at_least_one_backend: "backends.size() >= 1"
```

- `state` declares state variable names and types. Types match what `SpecCapture`
  getters return.
- `init` declares the initial state values (what `SpecSetup` constructor produces).
- `operations` declares each operation's inputs and optional outcome. No
  transition expressions — those are inferred from traces.
- `invariants` are proposed by `specgate propose-invariants` from observed
  traces. The user approves each one. Invariant expressions are opaque strings
  consumed by Quint generation.

## Multi-step test cases

For state machine specs, cases use `steps` instead of flat `inputs`/`expected`:

```yaml
cases:
  - name: register_then_run
    desc: Register a backend then run a spec
    steps:
      - operation: register_backend
        inputs: { name: rust }
        assert_state:
          backends: [mock, rust]
      - operation: run_spec
        inputs: { spec_path: fixtures/simple_pass.spec.yaml }
        expected:
          outcome: Complete
          report: { passed: 1, total: 1 }
```

Each step has:
- `operation` (required) — operation name from the `operations` section
- `inputs` (optional) — input values for this call
- `expected` (optional) — expected return value (partial match)
- `assert_state` (optional) — expected state after this step (partial match)

Partial matching: omitted fields in `expected` or `assert_state` are not checked.

Cases without `steps` use flat `inputs`/`expected` — backward compatible for
single-operation specs.

## Spec boundary rule

**One spec = one state boundary.** Operations that share state belong in the same
spec. Operations with independent state belong in separate specs. No spec
composition or include mechanism exists.

If a spec gets too big because too many operations share state, that is a signal
the component is too coupled — refactor the code. If there are just too many test
cases, split into case-only files:

```
specs/harness.core.spec.yaml           # state, operations, types, invariants
specs/harness.core.cases/
  happy_path.yaml                      # cases only
  error_handling.yaml
```
