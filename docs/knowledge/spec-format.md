# Spec file format

Spec files are YAML, validated by `spec-schema.json`. One file per component or
logical group of operations.

**File convention**: `specs/<name>.spec.yaml`

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Dotted component name, e.g. `core.validate` |
| `binding` | no | Binding declaration(s) — `{ name, target }` object or list of objects. `name` resolves to `bindings/<name>.yaml`, `target` selects the execution target. |
| `inputs` | cond | Named inputs with type/source/desc (single-operation specs) |
| `types` | no | Named type definitions (oneof, causes, or record) |
| `outcome` | cond | Outcome variants or single type (single-operation specs) |
| `outputs` | cond | Per-outcome output fields (single-operation specs) |
| `state` | no | State variables and types (StateMachine specs) |
| `init` | no | Initial state values (required when `state` is present) |
| `operations` | no | Named operations with inputs/outcomes (StateMachine specs) |
| `invariants` | no | Approved invariants (proposed by `specgate propose-invariants`) |
| `depends_on` | no | List of spec names this spec depends on for shared types |
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
  GeometryError:
    causes:
      NegativeDimension: { field: string }
      UnsupportedShape: { name: string }

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
    error: GeometryError
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
- `binding` is optional — overrides the spec-level binding target for this case

### Per-case binding target

Cases inherit the spec-level binding target by default. To use a different
target for specific cases, add a `binding` field:

```yaml
binding:
  name: rust
  target: extract-annotations  # default

cases:
  - name: stateless_extraction
    desc: Extracts annotation metadata from source
    inputs:
      source: "#[spec_operation(\"op_a\", kind = Stateless)]\nfn handler() {}"
    expected:
      outcome: Ok
      annotations:
        - SpecOperation: { operation: op_a, kind: Stateless }

  - name: capture_runtime
    desc: Capture annotation records field values at runtime
    binding:
      target: run-annotations  # different target for runtime tests
    inputs:
      source: "..."
    expected:
      outcome: Ok
      traces:
        - Capture: { operation: op_a, field: total_area, value: 78.54 }
```

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
name: geometry.canvas
binding:
  name: rust
  target: test-canvas

state:
  shapes: List<Shape>
  total_area: float

init:
  shapes: []
  total_area: 0.0

operations:
  add_shape:
    inputs: { shape: Shape }
  clear:
    inputs: {}

invariants:
  area_non_negative: "total_area >= 0.0"
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
  - name: add_shapes_updates_area
    desc: Adding shapes accumulates total area
    steps:
      - operation: add_shape
        inputs: { shape: { Circle: { radius: 5.0 } } }
        assert_state:
          total_area: 78.54
      - operation: add_shape
        inputs: { shape: { Rectangle: { width: 3.0, height: 4.0 } } }
        assert_state:
          total_area: 90.54
```

Each step has:
- `operation` (required) — operation name from the `operations` section
- `inputs` (optional) — input values for this call
- `expected` (optional) — expected return value (partial match)
- `assert_state` (optional) — expected state after this step (partial match)

Partial matching: omitted fields in `expected` or `assert_state` are not checked.

### Postconditions

Cases can include a `postconditions` field — a list of binding target invocations
to run after all steps complete. The harness resolves each postcondition's
`target` through the active binding, substitutes template variables from the
harness context (e.g., `{generated_test_path}`, `{workdir}`) into the
postcondition `inputs`, renders the binding target's `command`, and requires
the command to exit 0 for the case to pass.

```yaml
cases:
  - name: cleanup_verified
    steps:
      - operation: run_spec
        inputs: { spec_path: fixtures/example.spec.yaml }
        expected: { outcome: Complete }
    postconditions:
      - target: assert-file-absent
        inputs:
          path: "{generated_test_path}"
        desc: Generated test file removed after run
```

Postconditions are useful for asserting side effects (file absence, process
state, environment changes) that aren't captured in the operation's return value.

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
specs/geometry.canvas.spec.yaml           # state, operations, types, invariants
specs/geometry.canvas.cases/
  happy_path.yaml                         # cases only
  error_handling.yaml
```
