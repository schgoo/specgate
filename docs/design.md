# Spec-based verification system

Engineers write specs; LLMs implement them. No deterministic harness validates that
the implementation satisfies the spec — not at generation time, not during maintenance.
This system closes that gap.

---

## Core concepts

| Term | Definition |
|------|-----------|
| **Operation** | A named behavioral unit the spec makes claims about (e.g., `findUserByKey`). |
| **Claim** | A single assertion the spec makes about an operation. Each claim is one scorecard entry. |
| **Kind** | The operation's type: `Stateless`, `StateMachine`, `Sequence`, `ErrorMap`, `Structural`. Determines extraction and validation strategy. |
| **Role** | What an annotation contributes to the operation: `Setup`, `Checkpoint`, `Capture`, `Mock`. |
| **Context** | Ambient state an operation depends on (config, identity, feature flags) — not a direct parameter. |
| **Dependency** | An external service or non-deterministic source the operation interacts with. Bidirectional: outbound calls are outputs, responses are inputs. |
| **Measurable claim** | Has instrumentation. Validated by running N trials against a threshold. |
| **Narrative claim** | No instrumentation strategy exists. Human-reviewed, explicitly flagged. |

---

## Requirements

1. **Spec is the single source of truth.** If spec and implementation disagree, the implementation is wrong.

2. **Conformance checking is deterministic.** The harness is generated fully deterministically from annotations and the spec — no LLM in the generation or execution loop. An LLM may help *place* annotations, but once annotations exist, everything downstream is mechanical. Conformance is a per-claim scorecard (`47/50 satisfied`), not a binary gate.

3. **Conformance checking is continuous.** Runs in CI on every change. Spec drift is caught automatically.

4. **The spec covers diverse constraint types.** Behavioral (state machines), contractual (pre/post conditions), structural (dependency rules), temporal (ordering), resource (allocation budgets).

5. **Spec-to-code mapping is verifiable.** The glue between spec and code is explicit, reviewable, and testable — not a hand-written trust-me bridge.

6. **Minimal ceremony.** Overhead of making a spec executable is small relative to writing the spec itself.

7. **Incremental adoption.** Add specs per-module, no whole-codebase commitment.

8. **Language-agnostic spec, per-language enforcement.** Supports Rust and C# from day one. The spec is portable; the harness is language-specific.

9. **Claims are individually traceable.** Every assertion has an ID. Failures point to the specific spec clause violated.

10. **The spec models the environment.** Fault model (what a dependency *can* do), required response (what the system *must* do), enforcement (inject faults, assert responses).

11. **Varying formality is explicit.** Precise claims are machine-checked. Fuzzy claims are flagged as narrative — never silently ignored.

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  Spec layer (language-agnostic, shared source of truth) │
│                                                         │
│  Spec files ─── YAML (validated by JSON Schema)         │
│    Operations, types, outcomes, test cases              │
│    State declarations + approved invariants             │
│    Canonical format for all mechanisms                  │
│                                                         │
│  Quint models ── auto-generated from traces (optional)  │
│    State machines with invariants + temporal properties │
│    Format: .qnt files, never hand-edited                │
│    Transitions/guards inferred from observed traces     │
│                                                         │
│  Narrative ── ~10% of claims                            │
│    Human-reviewed only, not machine-checked             │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│  Compiler layer (per-language)                          │
│                                                         │
│  YAML compiler → parameterized tests (from cases)       │
│  Quint compiler → property tests (from .qnt, optional)  │
│  Rule compiler  → lint / static analysis commands       │
└────────────────────┬────────────────────────────────────┘
                     │
┌────────────────────▼────────────────────────────────────┐
│  Runtime layer (per-language)                           │
│                                                         │
│  1. Completeness check ─ annotation structure (seconds) │
│  2. Coverage check ──── claim ↔ annotation match (secs) │
│  3. Conformance check ─ run generated tests (minutes)   │
│  4. Trace collection ── ITF traces for Quint (optional) │
│  5. Scorecard ───────── unified conformance report      │
└─────────────────────────────────────────────────────────┘
```

Three mechanisms because a single format does not fit all claim types. The YAML
spec is the canonical source; Quint models are derived from observed execution
traces for formal verification. Test tables, structural rules, and state machine
properties are all expressed in the same YAML format.

### Three file types

| File | Purpose | Language-agnostic? |
|------|---------|-------------------|
| `specs/<name>.spec.yaml` | **What** — types, inputs, outputs, test cases | Yes |
| `bindings/<lang>.yaml` | **How** — build command, output path | No (per-language) |
| Source annotations | **Link** — connect code symbols to spec operation names | No (in-language) |

The spec declares behavior. The binding declares how to build and where artifacts
land. Source annotations mark which code implements which operations. The harness
joins them: spec's `binding` field → binding file → target definition → execute → assert.

### Binding files

**File convention**: `bindings/<language>.yaml`
**Schema**: `binding-schema.json`
**Linked from specs**: `binding: rust` → `bindings/rust.yaml`

Targets are either **command** (run a shell command) or **call** (invoke a function):

```yaml
# bindings/rust.yaml
language: rust
targets:
  build:
    command: cargo build -p fixture --message-format=json
    inputs:
      source:
        file: "{workdir}/fixture/src/lib.rs"
    outputs:
      file: "{workdir}/target/specgate-annotations.json"
      stderr: true
  test:
    command: cargo test -p specgate-core
    outputs:
      file: "{workdir}/target/specgate-captures.json"
  validate:
    call: specgate_core::validate
```

For command targets, inputs are delivered via `file`, `env`, or `arg`. For call
targets, inputs map directly to function parameters by name.

The binding does NOT contain discovery logic — the macros produce known
output in a known format at a known path. Operation names in the output match
operation names in the spec.

### Execution targets

Specs reference a **binding** file and a named **target** within it:

```yaml
# specs/rust.annotations.spec.yaml
name: rust.annotations
binding: rust       # → bindings/rust.yaml
target: build       # → targets.build in that binding
```

The spec stays language-agnostic. The binding owns the how — command, input
delivery, output reading.

### Spec file format

Spec files use YAML, validated by a JSON Schema. One file per operation (or per
logical group of related operations).

**File convention**: `specs/<name>.spec.yaml`

**Top-level fields**:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | str | yes | Dotted component name (e.g., `core.validate`) |
| `binding` | object or list | no | Binding declaration(s). Object `{ name, target }` for single-language, list of objects for multi-language. |
| `inputs` | map | no | Named, typed parameters (single-operation specs) |
| `types` | map | no | Named type definitions (oneof or record) |
| `outcome` | oneof/str | cond | Named outcome variants or single type (single-operation specs) |
| `outputs` | map | cond | Per-outcome output fields (keyed by `when <Variant>`) |
| `state` | map | no | State variables and types (StateMachine specs) |
| `init` | map | no | Initial state values (required when `state` is present) |
| `operations` | map | no | Named operations with inputs/outcomes (StateMachine specs) |
| `invariants` | map | no | Approved invariants (proposed by `specgate propose-invariants`) |
| `depends_on` | list | no | Spec names this spec depends on for shared types |
| `cases` | list | yes | Concrete test cases (≥1) |

A spec is either **single-operation** (has `inputs`/`outcome`/`outputs` at the top level)
or **multi-operation / state machine** (has `state`/`operations`). These are mutually
exclusive — a spec cannot have both `inputs` and `operations`.

Each input entry has `type` (required), plus optional `source` and `desc`.

**Input type declarations**: types can be declared inline or as named types.

*Inline* — for simple or one-off shapes:
```yaml
inputs:
  types:
    type: List[{ name: string, is_abstract: bool, has_generator: bool }]
```

*Named* — for complex or reused shapes (define in a top-level `types:` block):
```yaml
inputs:
  annotations:
    type: List[Annotation]

types:
  Annotation:
    oneof:
      SpecOperation: { operation: string, kind: string }
      SpecSetup: { operation: string, name: string, symbol: string, params: List[string] }
```

Both forms are valid. Use named types when the shape has variants (`oneof`), is
deeply nested, or appears in multiple specs. Use inline when the shape is flat
and only used once. Types are declared bottom-up: only include fields that test
cases actually exercise. New cases may require adding fields to the type.

**Types are suggestions, not rules.** Named types describe what fields must be
*available*, not how the implementation must structure its data. A generator is
free to use a Rust enum, a struct with an attribute field, a trait object, or
any other representation — as long as the test cases can construct inputs and
assert outputs using the declared fields. The spec constrains behavior, not
internal layout.

Each `cases` entry has:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Unique snake_case test name |
| `desc` | yes | Human-readable description |
| `inputs` | cond | Values for inputs (single-operation specs, or single-step state machine cases) |
| `expected` | cond | Expected outcome + outputs (single-operation, or per-step in state machine) |
| `steps` | cond | Ordered operation sequence (StateMachine specs only) |

A case uses either flat `inputs`/`expected` (backward-compatible single-operation) or
`steps` (multi-step state machine). These are mutually exclusive.

**Multi-step cases** (StateMachine specs only):

Each step has:

| Field | Required | Description |
|-------|----------|-------------|
| `operation` | yes | Operation name from the `operations` section |
| `inputs` | no | Input values for this operation |
| `expected` | no | Expected return value (partial match — omitted fields are not checked) |
| `assert_state` | no | Expected state after this step (partial match) |

The component lifecycle per case:

```
SpecSetup → creates component instance
SpecCapture → verify init state matches `init`
─────────────────────────────────────────────
Step 1: call operation, SpecCapture → read state
Step 2: call operation, SpecCapture → read state
  ...
```

No operation is inherently "first" — ordering comes from the test case. Guards
(inferred from traces in wave 2) capture the real ordering constraints.

**State machine spec example**:

```yaml
# yaml-language-server: $schema=../spec-schema.json
name: harness.core
binding: rust
target: test

# State variables — types match what SpecCapture getters return
state:
  backends: Set<string>

# Initial state — matches what SpecSetup constructor produces
init:
  backends: [mock]

# Operations — inputs and outcomes (no transition expressions)
operations:
  register_backend:
    inputs: { name: string }
  run_spec:
    inputs: { spec_path: string }
    outcome:
      oneof: [Complete, Error]

# Invariants — approved from wave 1 proposals
invariants:
  mock_always_registered: "mock ∈ backends"
  at_least_one_backend: "backends.size() >= 1"

types: ...
outputs: ...

cases:
  - name: register_then_run
    desc: Register a backend then run a spec
    steps:
      - operation: register_backend
        inputs: { name: rust }
      - operation: run_spec
        inputs: { spec_path: fixtures/simple_pass.spec.yaml }
        expected:
          outcome: Complete
          report: { passed: 1, total: 1 }
```

Key difference from single-operation specs: no `transition` or `guard` fields.
Those are inferred from traces and live in the generated Quint model. The YAML
has only what humans write and approve: state declarations, invariants, and cases.

**JSON Schema**: a `spec-schema.json` file provides editor autocompletion, inline
validation, and catches YAML parsing pitfalls (bare booleans, missing required
fields, wrong types). Editors (VS Code with Red Hat YAML extension, Rider,
IntelliJ) consume it natively via a `# yaml-language-server: $schema=` comment
or project-level configuration.

**Quint generation**: Quint models are generated from **observed execution traces**, not
directly from YAML. The user never writes Quint expressions. The pipeline:

1. Run annotated code → collect ITF traces via `SpecCapture` (wave 1)
2. Analyze traces → propose candidate invariants → user approves
3. Generate `.qnt` model from traces + approved invariants (wave 2)
4. `quint run` explores novel operation sequences, checking invariants
5. Export novel ITF traces → replay as proptests → collect more traces → refine

Transitions and guards are inferred from trace patterns (set-add, set-remove,
increment, assignment). Complex transitions fall back to `nondet` — Quint still
checks invariants. The YAML spec stores only what humans write and approve: state
declarations, invariant expressions, and test cases. Generated `.qnt` files are
never hand-edited — they are regenerated from traces on every change.

**Trace format**: Traces use Quint's **ITF (Informal Trace Format)** — JSON with
states and actions. Each test run produces one ITF trace:

```json
{
  "meta": { "spec": "harness.core", "test": "register_then_run" },
  "states": [
    { "backends": ["mock"] },
    { "backends": ["mock", "rust"] },
    { "backends": ["mock", "rust"] }
  ],
  "actions": [
    { "name": "init" },
    { "name": "register_backend", "inputs": { "name": "rust" } },
    { "name": "run_spec", "inputs": { "spec_path": "fixtures/simple_pass.spec.yaml" } }
  ]
}
```

The invariant proposer analyzes the `states` array across all traces to find
universal patterns (always-contains, never-empty, monotonic growth, implication,
bounded, idempotent).

---

## Workflows

### Translation (existing code → spec → new language)

```
1. Annotate source code (human + LLM guided)
2. Instrument annotated code (codegen, per Kind — deterministic)
3. Run existing tests against instrumented code → capture file
4. Generate proptests for expanded coverage (optional) → more captures
5. Combine captures → SPEC
6. Generate target-language code from spec, with annotations
7. Validate target implementation against spec
```

Steps 2–5 are fully deterministic. Step 1 is the only human-judgment step.

### New code (spec → implementation)

Write spec first → generate code with annotations → validate against spec.

Both workflows share the same spec format, annotation system, and validation harness.

### Two-wave architecture for state machines

State machine specs use a two-wave approach that infers formal properties from
observed behavior rather than requiring manual Quint expressions.

**Wave 1: Observe + Propose**

```
1. Annotate code with SpecOperation, SpecCapture, SpecSetup, SpecMock
2. Run existing tests with instrumentation enabled
3. Collect ITF traces — (state_before, operation, inputs, state_after) per step
4. Analyze traces — detect patterns, propose candidate invariants
5. User reviews — approve ("yes, this is a spec commitment") or reject each
6. Approved invariants go into the YAML spec
```

Invariant inference is deterministic pattern detection over collected data (not
stochastic). Patterns detected: always-contains, never-empty, monotonic growth,
implication, bounded, idempotent. May be wrong due to limited test coverage —
that is why the user signs off.

**Wave 2: Verify + Explore**

```
1. Generate Quint model from traces + approved invariants
2. Quint random simulation (quint run) explores novel operation sequences
3. Export novel ITF traces
4. Replay ITF traces as proptests against real code
5. Collect more traces from proptests → refine model → repeat
```

The virtuous cycle: more traces → better model → more exploration → more traces.
Wave 2 is optional — wave 1 alone provides value by formalizing observed behavior
into approved invariants.

---

## Annotation system

### Multi-place annotations

A single operation may be annotated in multiple places. Each annotation contributes
a piece — the harness collects all annotations sharing the same operation name.

Why: real code distributes an operation across constructors, base classes, properties,
and pipeline stages. A single-function annotation would require wrappers that do not
cover all patterns.

### State machine support

The existing five annotations are sufficient for state machines — no new annotations
needed. `SpecCapture` is the key enabler: it provides state observation between steps,
making trace collection possible for the two-wave architecture.

| Annotation | Role in State Machines |
|------------|------------------------|
| `SpecOperation` | Links each method to its operation name. Multiple per component. |
| `SpecSetup` | Creates the component instance before step sequences. |
| `SpecCapture` | **State observation.** Read before/after each step to build traces. Required for StateMachine kind. Can annotate a method (lens) or field (direct). |
| `SpecCheckpoint` | Within-step observation (intermediate state during a single operation). |
| `SpecMock` | Mock injection. Semantics unchanged for state machines. |

### Roles

| Role | Placed on | Contributes |
|------|-----------|-------------|
| `kind = Stateless\|StateMachine\|Sequence\|ErrorMap\|Structural` | Entry point method | Operation type + return value |
| `role = Setup` | Test fixture function (no `self`) | Construction + input values for the entry point's type |
| `role = Checkpoint` | Internal methods/call sites | Observable intermediate state |
| `role = State` | Fields/properties | State to snapshot before/after (StateMachine) |
| `role = Mock` | Methods calling external services | Makes function mockable; harness injects controlled responses |

Only one annotation per operation has a `kind` (the entry point). All others have a `role`.

Setup functions live in the test project. They construct the type under test and
provide input values. If the entry point is a free function, no setup is needed.
If it's a method and no setup is provided, `core.validate` warns about ambiguous
construction. Multiple setups per operation are allowed — test cases reference
which setup to use.

### Construction resolution

The harness works backwards from the entry point to build a construction graph:

1. **Entry point** — what types does it need? (self type, parameter types)
2. **For each type, try in order:**
   - Auto-discover a single public constructor → use it, recurse on its params
   - Find a `spec_setup` that returns this type → use it
   - Primitive/leaf type → provided by test case inputs
3. **If unresolvable** — validation error with actionable suggestion:
   > "Cannot construct `DirectoryKey` for operation `find_user`.
   > Add a `#[spec_setup("find_user")]` function that returns `DirectoryKey`."

All resolved constructor/setup parameters bubble up as flat test case inputs.
This is conceptually dependency injection resolution at code-generation time.
See also: `fundle` crate for potential runtime DI integration (future work).

Annotated code must be callable from the test project but need not be public API.
Each language has its own mechanism: C# uses `InternalsVisibleTo`, Rust uses
`pub(crate)` or `#[cfg(test)]` visibility.

### Example

```csharp
public class UsersRestRequest : BaseRequest {
    public UsersRestRequest(string tenant, string token) : base(tenant, token) { }

    public string TargetEndpoint { get; set; }

    [SpecOperation("findByKey", Kind = Sequence)]
    public async Task<User> FindByKeyAsync(DirectoryKey key) { ... }

    [SpecCheckpoint("findByKey")]
    internal string GetRoutingHint() { ... }

    [SpecMock("findByKey", Name = "restClient")]
    public Task<HttpResponse> CallApiAsync(HttpRequest req) { ... }
}

// In the test project:
[SpecSetup("findByKey", Name = "default")]
public static UsersRestRequest SetupRequest(string tenant, string token, string ep) {
    var r = new UsersRestRequest(tenant, token);
    r.TargetEndpoint = ep;
    return r;
}
```

Running the instrumented code against existing tests produces this extracted spec:

```yaml
operation: findByKey
kind: Sequence

inputs:
  tenant: string
  token: string
  target_endpoint: string
  key.type: oneof [ObjectId, Sid, LegacyDN, ProxyAddress]
  key.value: string

checkpoints:
  - routing_hint: string

output: User

cases:
  - inputs: { tenant: "contoso", token: "t1", target_endpoint: "/users",
              key.type: ObjectId, key.value: "abc123" }
    checkpoints: [{ routing_hint: "OID:abc123@contoso" }]
    output: { id: "abc123", display_name: "Alice" }

  - inputs: { tenant: "fabrikam", token: "t2", target_endpoint: "/users",
              key.type: Sid, key.value: "S-1-5-21" }
    checkpoints: [{ routing_hint: "SID:S-1-5-21@fabrikam" }]
    output: { id: "S-1-5-21", display_name: "Bob" }
```

Every field traces back to an annotation: `tenant`, `token`, and `target_endpoint`
from the setup function, `routing_hint` from the checkpoint,
`key.*` and the output from the entry point signature.

The Rust equivalent uses different types/structure but the same operation name and
roles. The harness maps by name, not by code shape.

### Kind determines extraction strategy

| Kind | Extraction | Spec output |
|------|-----------|-------------|
| Stateless | Capture (args, return_value) | Test table |
| StateMachine | Snapshot state before/after | State transitions |
| Sequence | Record ordered emissions | Checkpoint sequence |
| ErrorMap | Capture (args, error_variant) | Error classification |
| Structural | Static analysis (no runtime) | Deny/must rules |

### Completeness is statically checkable

| Kind | Required | Incomplete if |
|------|----------|---------------|
| StateMachine | ≥1 State annotation | No state registered |
| Sequence | ≥1 Checkpoint | No checkpoints |
| ErrorMap | ≥1 error-returning path | Method never errors |
| Stateless / Structural | Nothing beyond signature | Always complete |

CI fails fast on structural annotation errors without compiling.

---

## Execution model: inputs, contexts, dependencies

An operation has four kinds of external data:

1. **Direct inputs** — method parameters on the entry point.
2. **Contexts** — ambient state (config, identity, feature flags). Named groups of
   primitives declared in the spec. Each language implements a context provider that
   wires primitives into whatever mechanism the code uses (DI, static state, thread-locals).
3. **Dependencies** — external services or non-deterministic sources. Bidirectional:
   what the operation *sends* = observable output; what the dependency *returns* =
   controllable input.
4. **Outputs** — return values, checkpoints, dependency interactions.

### Context definition

```yaml
contexts:
  sdk_config:
    cafe_app_id: string
    base_uri: string
    enable_proxy_check: bool

operations:
  findByKey:
    context: [sdk_config]
    inputs:
      key.type: oneof [ObjectId, Sid, LegacyDN]
      key.value: string
    output: User
```

Context providers live in the test project and use existing test seams
(InternalsVisibleTo, mock frameworks). They are not production code.

### Dependency definition

```yaml
operation: findUserByKey
inputs:
  key.type: oneof [ObjectId, Sid]
  key.value: string
  rest_client.response.status: int       # dependency response = input
  rest_client.response.body: string
outputs:
  result: User
  rest_client.call.method: string        # dependency call = output
  rest_client.call.url: string
  rest_client.call.headers: map<string, string>
```

For multi-call dependencies (pagination, retries), the spec captures only wire-level
events — what crosses the boundary. Comparison is semantic (same key-value pairs),
not structural (same ordering).

Non-deterministic methods like `GenerateSessionId()` are annotated with `spec_mock`
on the method itself. The harness can replace them with controlled values.

### Testability diagnostic

| State | Meaning | Action |
|-------|---------|--------|
| Injectable | Constructor/property accepts the value | Normal input, vary freely |
| Seamed | Internal test seam exists (e.g., MockInstance) | Context provider in test project |
| Opaque | No injection path | Testability warning — recommend refactoring |

The system recommends making code more testable. As codebases improve, the contexts
section shrinks toward zero.

---

## Schema: decomposed primitives

The spec stores inputs/outputs as typed trees of primitives, extracted from source
types via reflection (C#) or proc macros (Rust).

```
DirectoryKey (C# class)             → Spec schema:
  ├── Type: enum {ObjectId, Sid}        key.type: oneof [ObjectId, Sid]
  └── Value: string                     key.value: string
```

Decomposition stops at: primitives (`int`, `string`, `bool`, `float`, `decimal`,
`bytes`), `oneof` types (leaf — a value from a fixed set of named variants),
collections (`list<T>`, `map<K,V>`). Types with public
fields/constructors recurse.

When the tool encounters a type it cannot construct, it blocks validation and
reports actionable guidance:

```
⚠ Cannot construct `OpaqueToken` — no public fields or constructor found.
  Validation for operation `findByKey` cannot run until resolved.
  Options:
  1. Add a public constructor with decomposable parameters.
  2. Annotate a factory method with [SpecGenerator]:
       [SpecGenerator("OpaqueToken")]
       static OpaqueToken CreateForSpec(string value) => new(value);
```

This is the same iterative guidance pattern: the tool does not silently skip
unconstructable types. It tells the user exactly what is blocking validation
and what to do about it.

Dotted paths (`key.type`, `context.tenant`) preserve semantic grouping and tell the
codegen backend which fields compose into a type.

### Native codegen — no serialization in the test path

The harness generates native constructor calls, not JSON. No serialization layer sits
between spec and implementation at test time — eliminating divergence from field casing,
null handling, or custom converters.

```
Spec: key.type = ObjectId, key.value = "abc"
C#:   new DirectoryKey(DirectoryKeyType.ObjectId, "abc")
Rust: DirectoryKey::new(KeyType::ObjectId, "abc")
```

### Input generation

| Strategy | When |
|----------|------|
| Captured data | Instrument existing tests → emit native literals |
| Auto-generated | From schema (random oneof variant, random string, etc.) |
| Constrained | `[Spec.Constraint]` narrows domain (regex, ranges) |
| Custom | `[Spec.Generator]` on a factory method |

Default is auto-generate from decomposed schema. Constraints are opt-in when
defaults produce invalid inputs.

---

## Claim validation

### Every measurable claim is statistical

All measurable claims flow through one pipeline: run N trials, report pass rate,
compare against threshold. Config per claim: `(trials, threshold)`.

| Claim | Trials | Threshold | Notes |
|-------|--------|-----------|-------|
| `computeRoutingHint("OID:abc@t-1")` | 1 | 100% | Deterministic (marked `exact`) |
| Fallback pipeline invariant | 1000 | 100% | State machine model checking |
| TinyLFU favors frequency over recency | 10000 | 95% | Statistical property |
| Response time p95 < 10ms | 5000 | 95% | Resource/timing claim |
| "UI feels responsive" | — | — | Narrative (human-reviewed) |

No distinction between "functional" and "non-functional" from the harness perspective.
Only the instrumentation strategy and `(trials, threshold)` differ. If a claim marked
`exact` shows < 100% pass rate, that is itself a bug signal.

### Scorecard

| Status | Meaning |
|--------|---------|
| PASS | Claim validated |
| FAIL | Claim violated |
| UNMAPPED | Spec makes verifiable claim, no annotations found |
| PARTIAL | Some operations annotated, others missing |
| NARRATIVE | Human-reviewed only (by design) |

The gap metric (UNMAPPED + PARTIAL) is itself a CI signal.

---

## Iterative annotation guidance

Unannotated code is a black box by design. The system suggests more annotations only
when it detects a problem.

### Under-annotation signals

| Signal | Detection |
|--------|-----------|
| Non-deterministic output | Re-run with same inputs, compare outputs |
| Uncaptured external calls | Network proxy and/or static call graph analysis |
| Uncaptured ambient state | Static analysis or runtime env comparison |
| Low spec coverage | Claim ↔ annotation matching |

### Over-annotation signals

| Signal | Detection |
|--------|-----------|
| Redundant checkpoint | Mutation testing: removing it still catches same bugs |
| Implementation-coupled | Spec fails on refactor but behavior unchanged |
| False dependency | Method annotated as dependency but is actually deterministic |

### Consistency signals

| Signal | Detection |
|--------|-----------|
| Ambiguous outcome | Method returns null or throws — tool flags and asks: Absence or RecoverableFailure? |
| Orphan annotation | Annotation references operation name not in the spec |
| Conflicting annotations | Two `kind` on one operation, or overlapping input names from different annotations |
| Spec drift | Re-extraction produces different data than existing spec file (diff reported, user decides) |
| Unconstructable type | Input type has no decomposition path or `[SpecGenerator]` — validation blocked until resolved |

### Flow

```
1. Annotate entry point only (minimal)
2. Extract spec, review scorecard
3. Scorecard: "non-deterministic output — annotate GenerateCorrelationId as dependency"
4. Add annotation, re-extract
5. Scorecard: "HTTP calls detected, no dependency annotated"
6. Add annotation, converge
```

---

## The spec only knows operations

The spec has no concept of classes, inheritance, polymorphism, or language constructs.
This dissolves most implementation-pattern gaps:

- **Virtual dispatch**: `UsersRestRequest` and `GroupsRestRequest` are different
  operations (`findUserByKey`, `findGroupByKey`). Base class is invisible to the spec.
- **Callbacks**: an input that affects output. The spec does not know it is a callback.
- **Cross-object state**: the spec sees the operation boundary, not the objects behind it.
- **Extension methods**: just another way to provide inputs.
- **Interior mutability**: the spec sees outputs, not internal mutation strategy.

---

## Spec boundaries = state boundaries

**Design principle: one spec = one state machine. No composition, no includes.**

Operations that share state belong in the same spec. Operations with independent
state belong in separate specs. Cross-spec interaction happens through inputs/outputs,
not shared state.

### Why no spec composition

If two operations share state, splitting them across specs creates coupling between
specs — spec B references spec A's state variables. That coupling is a design smell.
The Independence Axiom: if they're coupled, they're one functional unit.

### When a spec gets too big

Two sources of growth, two responses:

1. **Too many operations sharing state** — the component is too coupled. Refactor
   the code, not the spec format. SpecGate is telling you something.
2. **Too many test cases** — split cases into separate files:
   ```
   specs/harness.core.spec.yaml           # state, operations, types, invariants
   specs/harness.core.cases/
     happy_path.yaml                      # test cases only
     error_handling.yaml
     multi_backend.yaml
   ```

### SpecGate as a coupling detector

You try to spec your component, you find you can't split it without shared state
everywhere, and that is SpecGate telling you the component has too many
responsibilities. The spec boundary pressure is the same as "if it's hard to test,
the design is wrong" — but formalized into a concrete, measurable artifact.

### Shared types are not shared state

Components often share type definitions (data structures, schemas) across spec
boundaries. This is normal interface coupling, not the kind of state coupling that
forces specs together. Shared types are handled via `depends_on`:

```yaml
name: harness.rust
depends_on: [core.spec_document]
```

Types that cross boundaries get their own spec (e.g., `core.spec_document` defines
`SpecDocument`, `SpecCase`, etc.). Consumer specs declare the dependency. When the
types spec changes, `specgate validate` re-validates all downstream specs.

Without `depends_on`, following the shared-types chain would collapse all specs into
one — `harness.rust` shares `SpecDocument` with `harness.core` AND shares `Annotation`
with `rust.annotations`, merging everything. The DAG preserves modularity while making
contracts explicit.

---

## Cross-language abstractions

The spec must abstract over structural differences between C# and Rust. Key
distinctions the spec format accounts for:

**Ports (D1)**: The spec distinguishes *open ports* (consumer provides implementation
→ C# interface / Rust trait) from *closed ports* (component owns all implementations
→ C# interface+DI / Rust inner enum). This determines whether the port is public API
or internal testability.

**Outcome model (D2, D3, D13)**: The spec expresses outcome intent using `oneof`
with per-case output fields:

```yaml
outcome: oneof [Success, Absence, RecoverableFailure, Fatal, Cancellation]

outputs:
  when Success:
    user: User
  when RecoverableFailure:
    code: int
  when Absence:
    # no outputs — absence is the information
```

The spec does not prescribe where payload is stored — just what outcomes exist and
what is observable in each. Maps to C# exceptions / Rust Result+panic per the
compiler layer. Absence intent (C# `null`) maps to `Absence`, `RecoverableFailure`,
or `Fatal` depending on whether it is expected, an error, or a bug. Error identity
(machine-verifiable code) is separated from diagnostic detail (not spec-checked).

**Lifecycle (D5)**: The spec defines lifecycle operations (initialize, flush, release)
and their effects, not cleanup mechanisms (`IDisposable` vs `Drop`).

**Concurrency (D6)**: The spec cares about parallelism, blocking, serialization,
ordering, and cancellation semantics — not async mechanics (`Task` vs `Future`).

**Notifications (D7)**: Spec models signals with multiplicity, ordering, payload, and
reentrancy rules — not delegates/events vs closures/channels.

**Data (D8)**: "There is a thing with these properties." Not classes vs structs.

**Numerics/text (D9)**: Unicode scalar values for text, declared numeric domain and
overflow behavior. The spec type `str` is encoding-agnostic. When a wire boundary
requires a specific encoding (e.g., UTF-8), that is a dependency output constraint,
not a string type property.

**Collections (D10)**: Single-pass stream, replayable sequence, or materialized
collection. Not `IEnumerable` vs `Iterator`.

**Observability (D11)**: Structured emissions (level, event name, fields). Not
`ILogger` vs `tracing`.

**Test bridge (D12)**: Observable ports and capturable effects. Per-language harness
strategy.

### Spec type system

Abstract types by default; sized variants when claims depend on size:

| Abstract | C# | Rust |
|----------|-----|------|
| int | int | i32 |
| float | double | f64 |
| decimal | decimal | rust_decimal::Decimal |
| string | string | String |
| bool | bool | bool |
| bytes | byte[] | Vec\<u8\> |

Composites: `Option[T]`, `List[T]`, `Set[T]`, `Map[K, V]`, `oneof [A, B, C]`.
Defaults overridable per-claim.

---

## Supplementary mechanisms

**Ghost state for observables**: telemetry and side-effects tracked via spec-only
variables. The spec appends to `var telemetryLog: List[TelemetryEvent]`; the bridge
maps to real telemetry calls.

**Spec evolution**: spec lives in the repo, versioned in the same PR. The scorecard
diff is the evolution trail.

**Instrumentation**: Rust uses proc macros (compile-time). C# uses Roslyn source
generators (compile-time, deterministic). The source generator reads `[Spec*]`
attributes and emits instrumented wrappers that capture inputs, outputs, checkpoints,
and dependency interactions as JSON traces.

**Capture format**: JSON. Each capture is a JSON object containing `{operation,
inputs, contexts, dependency_calls, dependency_responses, checkpoints, outcome,
outputs}`. Multiple captures from a test run are written as a JSON array. Captures
from existing tests are merged into spec YAML `cases:` entries.

**C# type mapping**: Base types map per the spec type system table (line 564-577).
`Task<T>` unwraps to `T`. `Nullable<T>` and nullable reference types map to
`Option[T]`. Collection type mapping (`IEnumerable<T>` → `List[T]` vs `stream[T]`
per D10) is TBD — flag as unresolved in extraction diagnostics until decided.

**Multi-targeting**: The C# attribute library targets both `netstandard2.0` (for
net472 compat) and `net9.0`.

**Proptest integration**: default to `any::<T>()` from function signature. Annotations
can narrow domain with strategy syntax (regex patterns, numeric ranges).

---

## Prior art

Informed by Quint, TLA+, Alloy, Dafny, P language, OpenAPI, Cucumber, SPARK/Ada.

Key borrowings: Dafny's failure-compatible types and abstract modules; P's typed
event/observer model; SPARK's range-constrained numerics and ghost variables;
Quint Connect's trace replay; Alloy's multiplicity modifiers; TLA+'s fairness
conditions.

No existing tool covers cross-language spec enforcement, first-class cancellation,
open/closed port distinction, single-pass vs replayable collections, and local
observability together.

---

## Resolved questions

**Q6 (annotation syntax)**: C# syntax defined in `csharp-harness.md`. Rust syntax
deferred until needed.

**Spec file format**: YAML validated by JSON Schema. Quint auto-generated
from YAML for optional model checking.
