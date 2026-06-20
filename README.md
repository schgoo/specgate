# SpecGate

Deterministic spec-based verification for LLM-implemented code.

Engineers write specs. LLMs implement them. SpecGate closes the gap by providing
a non-stochastic harness that validates implementations against specs using
runtime traces.

## Current Status

- Spec format at **`spec_version: "0.4.0"`**
- Milestones 1-4 are effectively complete: annotations, runtime, harness core,
  comparison engine, and partial CLI
- Structured value matching with Mongo-inspired operators
- Multi-target bindings with spec-level and per-case target selection
- `#[derive(SpecEvent)]` for enums with unit and named-field variants
- `$run`, `$unordered`, and `$anywhere` directives
- `level`, `source`, and `async` support
- Property-based cases with generators, calls, and `$assert`
- **51 hand-written fixture tests passing**

## How It Works

```
Spec YAML (expected behavior)  +  Annotated Source Code
                    ↓
              SpecGate Harness
                    ↓
    Per-case results: expected vs actual traces
```

1. You write a **spec** — a YAML file declaring operations, test cases, and expected traces / assertions
2. You annotate **source code** with `#[spec_operation]`, `#[spec_event]`, `spec_trace!()`, etc.
3. The harness **generates tests**, runs them, collects traces, and compares

If the traces match, the implementation satisfies the spec. If not, you see exactly what diverged.

## Spec Format

```yaml
spec_version: "0.4.0"
name: fixture.statemachine_counter
binding: path/to/binding.yaml

operations:
  make_counter:
    kind: setup
  increment:
    outputs: [count]

cases:
  - name: increment_counter
    desc: Increment changes count from 0 to 1
    setup: make_counter
    operation: increment
    expected:
      - count: "0"
      - $run: increment
      - count: "1"

  - name: just_final_value
    desc: Only assert on the final observed value
    setup: make_counter
    operation: increment
    expected:
      - count: "1"
```

Expected is a list of assertions checked as a **subsequence** of the trace
stream. Include as much or as little as you care about — from a single value
to the full sequence of events. Canonical examples live under
`test/rust/crates/specgate-fixtures/specs/`.

## Annotations

| Annotation | Purpose | Trace emitted |
|---|---|---|
| `#[spec_operation("name")]` | Marks an operation to test | `Run { operation }` |
| `#[spec_setup("name")]` | Named constructor/factory | `Event` per argument |
| `#[derive(SpecEvent)]` | Enables structured emission for structs and enums | `Event { name, value }` for derived fields / variants |
| `#[spec_event]` | Observe field mutations | `Event { name, value }` |
| `spec_trace!("name", expr)` | Inline structured observation | `Event { name, value }` |
| `#[spec_mock("name")]` | Mock a call site | `Event` for request/response |

## Trace Model

Two event types:
- **`Event { name, value }`** — any observation (field capture, return value, mock interaction, checkpoint)
- **`Run { operation }`** — marks when an operation executes

`value` is structured, not just a string. The runtime preserves scalars and
collections as `Value` variants (`String`, `Integer`, `Float`, `Bool`, `List`,
`Map`, `Set`), which enables collection-aware assertions.

### Naming conventions
- `{operation}.{param}` — input parameters (e.g., `add.a`)
- `$result` — auto-generated return value assertion in specs
- `$outcome` — auto-generated result/option/panic outcome assertion in specs
- `$error` — auto-generated error assertion in specs
- `{field}` — struct field captures (e.g., `count`)
- `{mock}.request` / `{mock}.response` — mock interactions

### Comparison rules
- **`$run`** directives are strictly ordered (define the operation sequence)
- **`$unordered`** groups match a set of events in any order
- **`$anywhere`** groups assert items appear somewhere in the trace regardless of position
- Comparison is **subset**: all expected events must appear in actual, but extra events are allowed

## Operators

Structured values can be matched with Mongo-inspired operators:

- Equality / shape: `$eq`, `$type`, `$exists`, `$match`, `$not`
- Collections: `$size`, `$contains`, `$containsAll`, `$excludes`, `$any`, `$every`
- Strings / regex: `$matches`
- Numeric comparisons: `$gt`, `$gte`, `$lt`, `$lte`

```yaml
expected:
  - items:
      $size: 3
      $contains: "foo"
  - $result:
      $gt: 0
      $lt: 100
  - name:
      $matches: "^[A-Z]"
```

See `test/rust/crates/specgate-fixtures/specs/operators.spec.yaml`,
`scalar_operators.spec.yaml`, and `structured_output.spec.yaml`.

## Property Tests

Property cases use `kind: property` plus generators, named calls, and `$assert`
expressions:

```yaml
  - name: add_commutative
    kind: property
    runs: 100
    generators:
      a: i32[-1000, 1000]
      b: i32[-1000, 1000]
    calls:
      forward: { operation: add, inputs: { a: "{a}", b: "{b}" } }
      reversed: { operation: add, inputs: { a: "{b}", b: "{a}" } }
    expected:
      - $assert: "forward.$result == reversed.$result"
```

Supported generator families include numeric ranges, `bool`, bounded strings,
`oneof[...]`, plus `list[...]`, `set[...]`, `map[...]`, and `optional[...]`.
See `test/rust/crates/specgate-fixtures/specs/property_add.spec.yaml` and
`property_types.spec.yaml`.

## Bindings

Specs reference a binding file that points to the crate under test. Bindings
load all targets into a `BTreeMap`, so a spec can select a default `target:`
and individual cases can override it.

```yaml
# binding.yaml
language: rust
targets:
  default:
    package_root: ../path/to/crate
```

The spec's `binding:` field is an explicit file path (not a name convention).

## Project Structure

```
.specgate/                      # Snapshots and implementation workflow state
  snapshots/
specs/                          # Spec YAML files
  specgate.harness.spec.yaml    # The harness spec (35 test cases)
  core.spec_document.spec.yaml  # Spec format validation
  core.binding_document.spec.yaml
bindings/                       # Language binding files
spec-schema.json                # Root schema for .spec.yaml files
rust/                           # Main implementation workspace
  crates/specgate-types/        # Spec/binding parsing + validation
test/                           # Test fixtures (separate workspace)
  rust/crates/specgate-fixtures/
    src/                        # Annotated Rust source files
    specs/                      # Fixture spec YAML + binding
docs/
  design.md                     # Architecture and requirements
  knowledge/                    # Per-topic reference docs
  issues.md                     # Issue tracker
```

## Development Plan

### Milestones 1-4: Complete or effectively complete
- [x] `specgate-annotations` proc macro crate
- [x] `specgate-runtime` trace collection
- [x] Spec parsing, binding resolution, and harness execution
- [x] Structured trace comparison and reporting
- [x] Partial CLI (`run` / `validate`) and fixture coverage

### Milestone 5: In progress
- [ ] Broaden spec-as-code ergonomics
- [ ] Expand higher-level assertion helpers
- [ ] Improve authoring workflow around snapshots and generated coverage

### Milestone 6: In progress
- [ ] Extend multi-language support beyond the current Rust-first path
- [ ] Flesh out C# annotation / harness generation
- [ ] Stabilize shared trace interchange for cross-language bindings

## Design Principles

1. **Spec is the single source of truth**
2. **Conformance checking is deterministic** — no LLM in the verification loop
3. **Traces are the sole source of truth for conformance** — generated tests have zero domain knowledge
4. **The harness is a single contract** — one input (spec path), results out
5. **Validation artifacts are not implementation inputs** — trust boundary on generated code

See `docs/design.md` for the full requirements list.

## License

TBD


