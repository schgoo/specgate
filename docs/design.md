# SpecGate design

Engineers write specs; LLMs (or humans) implement them. SpecGate is the
deterministic harness that decides whether an implementation satisfies its
spec by comparing **runtime traces** against expected trace assertions.

No LLM sits in the verification loop. Annotations placed in source code
emit a stream of trace events at runtime; the harness matches the spec's
expected events against that stream as a **subsequence**.

---

## Core concepts

| Term | Definition |
|------|-----------|
| **Operation** | A named behavioral unit the spec makes claims about (e.g. `add`, `increment`, `withdraw`). Marked in source with `#[spec_operation("name")]`. |
| **Setup** | A named factory function that constructs the system-under-test for a case. Marked with `#[spec_setup("name")]`. |
| **Trace event** | One of two variants: `Event { name, value }` (a value observation) or `Run { operation }` (the boundary of an operation invocation). |
| **Expected assertion** | A single-entry map in a case's `expected:` list — either `{<name>: <value>}` (an Event match) or `{run: <operation>}` (a Run match). |
| **Subsequence match** | The expected assertions must appear in the actual trace stream in order. Arbitrary additional events between matches are allowed. Extra events at the end are allowed. |
| **Binding** | A YAML file (referenced by a path string in the spec) that connects the spec to a language-specific package. |

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

11. **Validation artifacts are not implementation inputs.** The harness produces test code and trace files to verify the implementation. These artifacts must never be used as inputs to the implementation process — doing so makes validation circular. The implementation agent's only sources of truth are: the spec, the binding, and the source code. This applies to both human and LLM implementers.

12. **Traces are the source of truth for conformance.** Runtime traces are the **only** source of actual values for test comparison. The generated test does not access fields, return values, or state directly — it drains the trace stream and compares it against the spec's expected traces. This keeps the generator completely generic: it has zero domain knowledge. If the spec wants to assert a value, the annotation must capture it as a trace event. Traces use a simplified two-variant model: `Event { name, value }` for all observations (captures, checkpoints, mock interactions, return values) and `Run { operation }` for operation execution. Position in the sequence determines before/after semantics.

13. **The harness is a single contract.** The harness takes two inputs: a spec (with test cases and expected traces) and annotated source code. It produces one output: test results showing expected vs actual traces for each case. How it works internally — annotation extraction, code generation, compilation — is an implementation detail, not a separate spec. One spec, two inputs, results out.

14. **Varying formality is explicit.** Precise claims are machine-checked. Fuzzy claims are flagged as narrative — never silently ignored.

---

## Architecture

```
┌────────────────────────────────────────────────────────────────┐
│  Inputs                                                        │
│                                                                │
│  spec YAML (cases, expected traces, binding path)              │
│  annotated source code  (#[spec_*] markers + runtime macros)   │
└──────────────────────────┬─────────────────────────────────────┘
                           │
┌──────────────────────────▼─────────────────────────────────────┐
│  1. Parse spec        — load YAML, validate against schema     │
│  2. Resolve binding   — read binding YAML, locate package      │
│  3. Discover symbols  — find annotated setups, operations,     │
│                         events, mocks in the source tree       │
│  4. Generate tests    — one test per case: call setup(s),      │
│                         invoke operation(s), drain trace stream│
│  5. Execute           — build & run; runtime macros emit       │
│                         Event/Run records to a thread-local    │
│                         buffer                                 │
│  6. Subsequence match — for each case, walk expected list and  │
│                         actual trace list together; gaps OK    │
│  7. Report            — per-case pass/fail + full trace stream │
└────────────────────────────────────────────────────────────────┘
```

The harness has zero domain knowledge. Every assertion is an entry in
`expected:`; every actual value is a trace event captured by an annotation.
The matcher is a generic ordered-subsequence walk over `(name, value)` and
`run` records.

### Three file types

| File | Purpose | Language-agnostic? |
|------|---------|--------------------|
| `<name>.spec.yaml` | **What** — cases, expected trace assertions, binding pointer | Yes |
| `<binding>.yaml` (referenced as a path) | **How** — language, package root, optional command/function targets | No |
| Annotated source files | **Link** — connect code symbols to spec operation/setup/event/mock names | No |

The spec is language-agnostic. The binding points at the language-specific
package. Source annotations name the operations and events the spec talks
about. The harness joins them.

---

## Spec file shape

A spec is a YAML document with this top-level shape:

```yaml
spec_version: "0.4.0"
name: fixture.statemachine_counter   # dotted component name (required)
binding: binding.yaml                # path to binding file (relative to this spec)

operations:
  make_counter:
    kind: setup
  increment:
    outputs: [count]

cases:                               # required, non-empty
  - name: increment_once
    desc: Incrementing counter from 0 produces count 1
    setup: make_counter              # optional — names a #[spec_setup] fn
    operation: increment             # required for single-step cases
    inputs: { initial: 0 }           # optional — values passed to setup/operation
    expected:                        # required — list of single-entry maps
      - count: "0"                   # Event match: name=count, value="0"
      - $run: increment              # Run match: operation=increment
      - count: "1"
```

### `binding`

Always a **string path** to a binding YAML file, relative to the spec
file. The binding declares the language and the package root.

```yaml
# binding.yaml referenced by the spec above
language: rust
targets:
  default:
    package_root: ..
```

### `cases`

A list of at least one test case. Each case has:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Unique snake_case identifier within the file |
| `desc` | yes | Human-readable description |
| `setup` | no | Setup function name (string) or map of `{alias: setup_fn}` for multi-setup cases |
| `operation` | yes for single-step cases | Operation name to invoke |
| `steps` | yes for multi-step cases | Ordered list of `{operation: <name>}` entries |
| `inputs` | no | Values bound to setup/operation parameters by name |
| `expected` | yes | List of single-entry maps, matched as a subsequence of the trace stream |

`operation` and `steps` are mutually exclusive within a case.

### `expected` — list of single-entry maps

Each entry is either:

- `{<name>: <value>}` — matches an `Event` with that `name` and stringified `value`
- `{$run: <operation>}` — matches a `Run` for that operation
- `{$unordered: [...]}` — matches contained items in any order
- `{$anywhere: [...]}` — matches contained items anywhere in the stream

Values are always stringified for comparison (the harness coerces both
sides). The list is matched as an **in-order subsequence** of the actual
trace stream — see "Subsequence matching" below.

### Multi-step cases

For cases that exercise more than one operation against the same
setup, use `steps:` instead of `operation:`:

```yaml
- name: increment_then_decrement
  desc: Count goes 0 to 1 to 0
  setup: make_counter
  steps:
    - operation: increment
    - operation: decrement
  expected:
    - count: "0"
    - run: increment
    - count: "1"
    - run: decrement
    - count: "0"
```

The `expected:` list is still a single flat subsequence across all steps.

### Multi-setup cases

When an operation takes multiple objects, declare them as a map of
aliases to setup function names. The aliases become parameter names for
the operation:

```yaml
- name: transfer_between_accounts
  setup:
    source: make_source
    target: make_target
  operation: transfer
  inputs: { amount: 50 }
  expected:
    - source.balance: "100"
    - target.balance: "0"
    - run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

See `test/rust/crates/specgate-fixtures/specs/multi_setup.spec.yaml`.

### Mocks in `inputs`

For cases that exercise a `#[spec_mock]` site, the table of responses is
provided under the mock's name in `inputs`:

```yaml
- name: find_user_1
  setup: make_service
  operation: get_user
  inputs:
    id: "user_1"
    db:                     # name of the spec_mock
      "user_1": "Alice"     # input → mocked response
  expected:
    - run: get_user
    - db.request: "user_1"
    - db.response: "Alice"
    - get_user.result: "Alice"
```

See `test/rust/crates/specgate-fixtures/specs/mock_field.spec.yaml`.

---

## Trace model

The runtime emits a flat stream of events; there are exactly two variants:

```
Event { name: string, value: string }   // any value observation
Run   { operation: string }              // the boundary of an operation call
```

Everything observable — field mutations, return values, mock requests,
mock responses, inline checkpoints, setup arguments — is an `Event`.
Operation entries are `Run`. Position in the sequence is significant
(events before a `Run` for op X are "pre-X"; events after are "post-X")
but the spec author never has to think in those terms — they simply list
the events they care about, in order.

### Subsequence matching semantics

Given `expected = [e1, e2, e3, …]` and `actual = [a1, a2, …]`, the case
passes iff there exist indices `i1 < i2 < i3 < …` such that
`ej == actual[ij]` under entry-wise equality. In English:

- Every expected entry must appear in the actual trace.
- Order is preserved.
- **Gaps are allowed** — additional events may appear between matches.
- **Trailing events are allowed** — the actual stream can be longer.

This lets specs assert as much or as little as they care about. A spec
that only cares about the final value of `count` can write
`expected: [- count: "1"]`. A spec that cares about the full trajectory
lists every event in order. The matcher behaviour is the same.

Reference fixtures:

- `subsequence_with_gaps.spec.yaml` — asserts only the first and last
  value of a counter that increments twice; the intermediate `count: "1"`
  is intentionally omitted.
- `subsequence_wrong_order.spec.yaml` — proves the matcher rejects an
  out-of-order expectation list.
- `mismatch_missing_event.spec.yaml` — proves the matcher rejects an
  expectation absent from the trace.

---

## Annotations

Five annotations cover the entire model. Every fixture in
`test/rust/crates/specgate-fixtures/src/` uses only these five.

| Annotation | Placed on | Effect | Trace emitted |
|------------|-----------|--------|---------------|
| `#[spec_operation("name")]` | Free function or method | Marks the operation the spec invokes. | `Run { operation: name }` at the entry point. |
| `#[spec_setup("name")]` | Free function (no `self`) | Names a factory the case can invoke by `setup:`. | `Event { name: "<setup>.<param>", value }` per parameter. |
| `#[spec_event]` | Struct field | Every write to the field emits an event. | `Event { name: "<field>", value: new_value }` on each mutation. The field name is the trace name; multi-setup cases get the alias prefix (e.g. `source.balance`). |
| `spec_event!("name", expr)` | Inline expression | Records the value of `expr` at this point in execution. | `Event { name, value: format!("{}", expr) }`. |
| `#[spec_mock("name")]` | Local binding around a method call | Replaces the call with the case's mock table lookup. Emits both the request and the response. | `Event { name: "<mock>.request", value: input }`, then `Event { name: "<mock>.response", value: mocked_response }`. |

**No `kind` parameter.** `#[spec_operation]` takes a single name; the
shape of the operation (pure, stateful, multi-step, error-returning…) is
expressed entirely by what events the spec lists in `expected:`.

**No `spec_capture` or `spec_checkpoint!()`.** Field capture is
`#[spec_event]`; inline capture is `spec_event!()`. Those two names
cover every observation pattern in the fixtures.

### Naming conventions used by fixtures

- `<operation>.<param>` — input parameters of an operation (`add.a`).
- `<operation>.result` — return value (`add.result`).
- `<operation>.outcome` — `Ok` / `Error` for `Result<T,E>` returns.
- `<operation>.error` — error message string for the `Err` arm.
- `<field>` — bare field name for `#[spec_event]` captures on the
  single-setup case (`count`, `balance`).
- `<alias>.<field>` — field captures under a multi-setup alias
  (`source.balance`, `target.balance`).
- `<mock>.request` / `<mock>.response` — mock interactions.
- `<setup>.<param>` — setup arguments (`make_counter.initial`).

These are conventions enforced by the runtime macros, not by the
matcher. The spec author just writes the names; the matcher compares
strings.

### Composition

All annotations sharing the same operation name are collected into one
operation. A method can carry `#[spec_operation]` and a containing
struct can carry `#[spec_event]` fields — both contribute to the trace
stream when that operation runs.

```
#[spec_operation("transfer")]          ─┐
#[spec_event] on Account.balance       ─┼─► one operation "transfer"
#[spec_mock("renderer")]                ─┘
```

---

## Harness execution flow

Per spec:

1. **Parse** the YAML; validate against `spec-schema.json`.
2. **Resolve binding** — read the file pointed to by `binding:`.
3. **Discover annotations** — locate `#[spec_setup]`/`#[spec_operation]`
   functions and `#[spec_event]`/`#[spec_mock]` markers in the package.
4. **Generate tests** — one test per case. The generated test:
   - calls the named `setup` (passing `inputs` by name)
   - invokes `operation` (or each entry in `steps`) on the result
   - drains the runtime's trace buffer
5. **Build & run** the generated tests against the package.
6. **Subsequence-match** each case's `expected:` list against its captured
   trace stream.
7. **Report** — per-case `pass`/`fail` plus the full actual trace stream
   (so downstream tooling can diff, replay, or render reports).

The runtime stores trace events in a thread-local buffer. The generated
test takes ownership of that buffer between operation invocations; this
is what makes the harness reentrant across cases without coordination.

---

## Spec boundaries = state boundaries

Operations that share state belong in the same spec. Operations with
independent state belong in separate specs. There is no spec composition
or include mechanism; cross-spec coupling happens at the type level via
`depends_on:` only.

If a spec gets too large because too many operations share state, the
component is too coupled — refactor the code, not the spec format. If
it gets large because of test case count alone, split cases into
separate files in a sibling directory.

---

## Cross-language notes

The spec is YAML; the binding picks a backend (`language: rust` or
`csharp`). Annotation surface matches across languages:

| Spec concept | Rust | C# |
|--------------|------|-----|
| Operation | `#[spec_operation("name")]` | `[SpecOperation("name")]` |
| Setup | `#[spec_setup("name")]` | `[SpecSetup("name")]` |
| Field event | `#[spec_event]` on field | `[SpecEvent]` on property |
| Inline event | `spec_event!("name", expr)` | `SpecEvent.Record("name", expr)` |
| Mock | `#[spec_mock("name")]` | `[SpecMock("name")]` |

C# support is planned; current fixtures are Rust-only.

---

## Reference fixtures

The canonical examples of every supported pattern live in
`test/rust/crates/specgate-fixtures/`. Each fixture is a pair: a
`.spec.yaml` and a `.rs` implementing it.

| Pattern | Fixture |
|---------|---------|
| Pure function | `stateless_add` |
| Counter / state machine | `statemachine_counter` |
| Multi-step case | `multi_step` |
| Multiple captured fields | `multi_field_capture` |
| Inline checkpoint | `checkpoint_inline` |
| Mock interception | `mock_field`, `mock_multi_response`, `mock_not_found` |
| Setup with parameters | `setup_with_params` |
| Multiple setups | `multi_setup` |
| Result Ok / Err | `result_ok`, `result_err` |
| Panic / unrecoverable | `unrecoverable` |
| Void-returning operation | `void_operation` |
| Nested operation calls | `nested_operations` |
| Subsequence with gaps | `subsequence_with_gaps` |
| Read-only operation | `readonly_operation` |
| Multiple cases per spec | `multi_case` |
| Out-of-order expectation (fails) | `subsequence_wrong_order` |
| Missing expected event (fails) | `mismatch_missing_event` |

When a doc and a fixture disagree, the fixture is authoritative.

---

## Prior art

Informed by Quint, TLA+, Alloy, Dafny, P language, OpenAPI, Cucumber,
SPARK/Ada. Key borrowing: P's typed event/observer model, Quint
Connect's trace replay.
