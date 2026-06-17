# SpecGate

Deterministic spec-based verification for LLM-implemented code.

Engineers write specs. LLMs implement them. SpecGate closes the gap by providing
a non-stochastic harness that validates implementations against specs using
runtime traces.

## How It Works

```
Spec YAML (expected behavior)  +  Annotated Source Code
                    ↓
              SpecGate Harness
                    ↓
    Per-case results: expected vs actual traces
```

1. You write a **spec** — a YAML file declaring operations, test cases, and expected traces
2. You annotate **source code** with `#[spec_operation]`, `#[spec_event]`, etc.
3. The harness **generates tests**, runs them, collects traces, and compares

If the traces match, the implementation satisfies the spec. If not, you see exactly what diverged.

## Spec Format

```yaml
name: my.component
binding: path/to/binding.yaml

cases:
  - name: basic_add
    operation: add
    inputs: { a: 2, b: 3 }
    expected:
      add.result: "5"

  - name: increment_counter
    setup: make_counter
    operation: increment
    expected:
      count: "1"
```

## Annotations

| Annotation | Purpose | Trace emitted |
|---|---|---|
| `#[spec_operation("name")]` | Marks an operation to test | `Run { operation }` |
| `#[spec_setup("name")]` | Named constructor/factory | `Event` per argument |
| `#[spec_event]` | Observe field mutations | `Event { name, value }` |
| `spec_event!("name", expr)` | Inline observation | `Event { name, value }` |
| `#[spec_mock("name")]` | Mock a call site | `Event` for request/response |

## Trace Model

Two event types:
- **`Event { name, value }`** — any observation (field capture, return value, mock interaction, checkpoint)
- **`Run { operation }`** — marks when an operation executes

### Naming conventions
- `{operation}.{param}` — input parameters (e.g., `add.a`)
- `{operation}.result` — return value (e.g., `add.result`)
- `{operation}.outcome` — Ok/Error/Unrecoverable
- `{operation}.error` — error message
- `{field}` — struct field captures (e.g., `count`)
- `{mock}.request` / `{mock}.response` — mock interactions

### Comparison rules
- **Run** events are strictly ordered (define the operation sequence)
- **Event** entries between Run markers are compared as **sets** (field iteration order doesn't matter)
- Comparison is **subset**: all expected events must appear in actual, but extra events are allowed

## Bindings

Specs reference a binding file that points to the crate under test:

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
specs/                          # Spec YAML files
  specgate.harness.spec.yaml    # The harness spec (35 test cases)
  core.spec_document.spec.yaml  # Spec format validation
  core.binding_document.spec.yaml
bindings/                       # Language binding files
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

### Milestone 1: Annotations + Runtime ✦ *in progress*
- [ ] `specgate-annotations` proc macro crate (Event/Run model)
- [ ] `specgate-runtime` trace collection (thread-local store, drain)
- [ ] Fixture crate compiles with annotations

### Milestone 2: Harness Core
- [ ] Parse spec YAML, resolve binding
- [ ] Extract annotations from source crate
- [ ] Generate test code from spec cases + annotations
- [ ] Compile and run generated tests
- [ ] Collect traces from test output

### Milestone 3: Comparison Engine
- [ ] Subset matching (expected ⊆ actual)
- [ ] Set comparison between Run markers
- [ ] Strict Run ordering
- [ ] Report: expected vs actual per case

### Milestone 4: CLI
- [ ] `specgate run <spec-path>` — run a spec end-to-end
- [ ] `specgate validate <spec-path>` — check spec structure
- [ ] JSON output for CI integration

### Milestone 5: Spec-as-Code Library (exploratory)
- [ ] `CaseBuilder` API for Rust-native spec definitions
- [ ] Cross-language stub generation from Rust specs
- [ ] Rich assertion helpers (temporal, call counts)

### Milestone 6: C# Support
- [ ] C# annotation attributes
- [ ] C# test generator
- [ ] Shared trace format (JSON interchange)

## Design Principles

1. **Spec is the single source of truth**
2. **Conformance checking is deterministic** — no LLM in the verification loop
3. **Traces are the sole source of truth for conformance** — generated tests have zero domain knowledge
4. **The harness is a single contract** — one input (spec path), results out
5. **Validation artifacts are not implementation inputs** — trust boundary on generated code

See `docs/design.md` for the full requirements list.

## License

TBD
