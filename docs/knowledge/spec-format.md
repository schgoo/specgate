# Spec file format

Spec files are YAML, validated by the root `spec-schema.json`. One spec
file per component (or logical group of operations that share state).

**File convention**: `<name>.spec.yaml` (e.g.
`test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml`).

The canonical examples of every supported pattern live under
`test/rust/crates/specgate-fixtures/specs/`. When this doc and a fixture
disagree, the fixture is the source of truth.

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `spec_version` | yes | Schema version string, currently `"0.4.0"` |
| `name` | yes | Dotted component name, e.g. `fixture.stateless_add` |
| `binding` | no | Path (string) or list of paths to binding YAML files |
| `target` | no | Default binding target for the whole spec |
| `operations` | yes | Named operations, each declaring its own inputs/outputs/outcome |
| `cases` | yes | List of test cases |
| `types` | no | Named type definitions shared across operations |
| `depends_on` | no | List of other spec names this spec depends on |

## `spec_version`

Required. The harness checks this to determine which spec format to
parse. Current version is `"0.4.0"`.

```yaml
spec_version: "0.4.0"
```

## `binding`

A string path (single implementation) or a list of paths
(multi-implementation conformance testing):

```yaml
# Single binding
binding: binding.yaml

# Multiple bindings
binding:
  - bindings/rust.yaml
  - bindings/csharp.yaml
```

The harness reads the binding file to learn the language, package
location, and how to resolve operations. See `docs/knowledge/bindings.md`.

## `target`

Optional default target within the binding file:

```yaml
binding: binding_multi.yaml
target: alt
```

This selects one entry from the binding's `targets:` map. A case can still
override it with its own `target:` field. Canonical fixtures:
`target_selection.spec.yaml`, `per_case_target.spec.yaml`.

## `operations`

Each operation declares its inputs and outputs. Operations are keyed by
name:

```yaml
operations:
  add:
    inputs: { a: i32, b: i32 }
    outputs: [$result]
```

### Inputs

`inputs` is a map of parameter names to type references. A value is a scalar
type (`i32`, `string`, …), a named type, or an inline complex type. Case
`inputs:` bind values to these parameters by name.

#### Optional inputs via `default:`

An input written as a mapping with a `default:` key becomes **optional**: a
case may omit it and the harness supplies the declared default. Inputs without
`default:` remain required. Defaults are materialized like any case input, so
both scalar and complex/named-type defaults work:

```yaml
types:
  Offset: { dx: i32, dy: i32 }

operations:
  scale:
    inputs:
      value: i32
      factor:
        type: i32
        default: 2            # scalar default → optional
    outputs: [$result]
  shift:
    inputs:
      base: i32
      by:
        type: Offset
        default: { dx: 1, dy: 1 }   # complex default → optional
    outputs: [$result]
```

A case that omits the defaulted input uses the default; providing it overrides
the default:

```yaml
- name: uses_default_factor
  operation: scale
  inputs: { value: 5 }         # factor defaults to 2 → 10
  expected:
    - $result: "10"
- name: explicit_factor
  operation: scale
  inputs: { value: 5, factor: 3 }   # override → 15
  expected:
    - $result: "15"
```

Canonical fixture: `default_input.spec.yaml`.

### Outputs

Outputs is a list of event names the operation can produce. Each item is
either a bare string (simple event) or a map with type / enum info:

```yaml
# Simple — bare event names plus auto-generated result events
outputs: [count, balance, $result]

# With types
outputs:
  - $result: i32
  - count: i32

# With enum variants and associated data
outputs:
  - $outcome:
      oneof:
        Complete:
          results: List<CaseResult>
        Error:
          reason: string
```

The harness validates that `expected` in a case only asserts on events
declared in the operation's outputs.

Auto-generated harness fields always use the `$` prefix:

- `$run` — an operation invocation boundary (`{$run: <operation>}`)
- `$result` — return value
- `$outcome` — result / option / panic outcome (`Ok`/`Error`, `Some`/`None`, …)
- `$error` — error payload for the `Err` arm of a `Result`
- `$fault` — panic / unwind message for an unrecoverable operation
  (canonical fixture: `unrecoverable.spec.yaml`)

User-authored capture names stay bare (`count`, `shape.radius`,
`db.request`).

### Operation kinds

| Kind | Description |
|------|-------------|
| *(default)* | Regular annotated operation (discovered via `#[spec_operation]`) |
| `command` | Shell command — exit 0 = pass |

```yaml
operations:
  increment:
    outputs: [count]
  mechanism_proof:
    kind: command
    desc: Runs cargo test --test mechanism_proof.
    outputs:
      - $outcome:
          oneof:
            Complete: {}
            Error: {}
```

### Async operations

Set `async: true` when the implementation entry point is asynchronous:

```yaml
operations:
  fetch:
    async: true
    inputs: { url: string }
    outputs: [$result]
```

The implementation must use an async operation entry point. The generated
runner drives the operation's future to completion with a single top-level
runtime entry; the runtime is chosen by the binding target's `runtime:` field
(`smol` by default, or `tokio` — see [bindings.md](bindings.md#async-runtime)).
Reactor-backed futures (timers, I/O) need the matching runtime declared on the
target. Canonical fixtures: `async_fetch.spec.yaml` (trivial async),
`async_smol_timer.spec.yaml` and `async_tokio_timer.spec.yaml` (reactor-backed).

## Type reference

Type references appear in operation `inputs`, `outputs`, and named `types`. A
reference is either a **string** (a scalar built-in or a named type) or an
**inline object** (a complex/collection type).

### Scalar built-ins

| Type | Meaning |
|------|---------|
| `string` | UTF-8 string |
| `i32`, `i64` | Signed integers |
| `f64` | Floating point |
| `bool` | Boolean |
| `value` | The universal structured value (see below) |

### `value` — the universal structured type

`value` is a **first-class built-in**: the same value lattice the runtime uses
for every trace observation. A `value` is any scalar, or a `List`, `Map`, or
`Set` of `value`. Use it when an output/field is a heterogeneous or
open-ended structured value rather than a fixed shape.

The matcher compares a `value` by its runtime kind:

- **strings** — by equality
- **lists** — as an ordered subsequence (or in any order with `$unordered`)
- **maps** — by matching each asserted key and its value
- **sets** — by presence (membership), order-independent
- an **absent event** represents a null/`None` field

Because the runtime `Value` is the spec's own value lattice, `value` cannot
itself be modelled as a `SpecEvent` type — it is built in. Source of truth:
the `value` entry in `spec-schema.json` and
`specs/specgate.harness.spec.yaml` (the harness models its own `TraceEvent`
values as `value`).

### Complex / collection types (inline objects)

| `type:` | Extra keys | Meaning |
|---------|-----------|---------|
| `list` | `items` | Ordered list of the item type |
| `set` | `items` | Unordered unique collection |
| `map` | `keys`, `values` | Keyed collection |
| `optional` | `items` | Optional value |
| a scalar | — | Same as the bare scalar string form |

```yaml
outputs:
  - readings: { type: list, items: i32 }
  - attributes: { type: map, keys: string, values: string }
```

### Named types

Define reusable structs and enums under `types:` and reference them by name.
See [authoring.md](authoring.md#defining-types) for struct/enum syntax and
guidance on when to decompose into primitives instead.

## `cases`

### Concrete runnable cases

```yaml
cases:
  - name: add_2_3
    desc: Adding 2 + 3 returns 5
    operation: add
    inputs: { a: 2, b: 3 }
    expected:
      - $result: "5"
```

| Case field | Required | Description |
|------------|----------|-------------|
| `name` | yes | Snake_case identifier, unique within the file |
| `desc` | recommended for all, expected for concrete and narrative cases | Human-readable description |
| `kind` | no | Defaults to a concrete runnable case; `narrative` and `property` are special forms |
| `operation` | for single-step concrete case | Operation name (must match the operations block) |
| `steps` | for multi-step | Ordered list of `{operation, inputs?, expected?}` |
| `inputs` | no | Values bound to operation parameters by name |
| `expected` | yes | Expected trace assertions (see below) |
| `target` | no | Per-case binding target override |
| `level` | no | Normative strength: `must`, `should`, or `may` |
| `source` | no | Provenance metadata for reporting tools |

`operation` and `steps` are mutually exclusive on a single case.

### `level` and `source`

Concrete cases can carry normative strength and provenance metadata:

```yaml
- name: add_with_provenance
  desc: Case with source provenance metadata
  level: must
  source:
    assertion_ids: [TEST-A1, TEST-A2]
    spec: "Test Specification v1.0"
    section: "§3.1"
  operation: add
  inputs: { a: 2, b: 3 }
  expected:
    - $result: "5"
```

- `level: must` — missing implementation is an error
- `level: should` — missing implementation is a warning
- `level: may` — missing implementation may be skipped

Canonical fixtures: `provenance_example.spec.yaml`,
`level_should_missing.spec.yaml`, `level_may_missing.spec.yaml`.

### Property cases

Property cases execute the same operation pattern repeatedly with generated
inputs. They use `kind: property` and named `calls:` plus `$assert`
expressions.

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

| Field | Required | Description |
|-------|----------|-------------|
| `kind` | yes | Must be `"property"` |
| `runs` | yes | Number of generated runs |
| `generators` | yes | Named generator expressions |
| `calls` | yes | Named operation invocations using generated placeholders |
| `expected` | yes | List of `$assert` expressions |

Supported generator shapes include:

- `i32[min, max]`, `f64[min, max]`
- `bool`
- `string[min_len, max_len]`
- `string[min, max, pattern: "regex"]`
- `oneof["a", "b", "c"]`
- `list[element_type, len: min..max]`
- `set[element_type, size: min..max]`
- `map[key_type, value_type, size: min..max]`
- `optional[type]`

On failure, the harness reports a `counterexample` and keeps only the
failing run's `traces`. Canonical fixtures: `property_add.spec.yaml`,
`property_types.spec.yaml`, `property_counterexamples.spec.yaml`.

### Narrative cases

Narrative cases express implementation constraints — they are read by
agents but not executed by the harness:

```yaml
  - name: no_source_interpretation
    kind: narrative
    desc: >
      The harness must not interpret Rust source with syn.
    verify:
      - Confirm no syn-based expression evaluation in harness source
      - The harness should invoke cargo build/test, not evaluate in-process
```

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Snake_case identifier |
| `kind` | yes | Must be `"narrative"` |
| `desc` | yes | Constraint in plain language |
| `verify` | no | Steps to manually verify the constraint |

### Setups are invisible

Setups never appear in a trace: a setup is never an operation (`$run`) and
emits no setup-named events. A spec describes only operations, their inputs,
and their observable outputs — it never names a constructor or declares
`kind: setup`. How an operation's receiver or state objects get built is an
implementation concern, resolved from code. A setup's construction inputs are
declared as ordinary operation inputs and supplied in the case's `inputs:` (see
below); the harness routes them to the setup by name.

In code, a setup is a function annotated with the **operation** it prepares:

```rust
#[spec_setup("increment")]            // linked to the operation, not named in the spec
fn make_counter() -> Counter { Counter { count: 0 } }
```

The harness matches a setup's return value to the operation's method receiver
or a parameter **by type**. The spec just runs the operation:

```yaml
- name: increment_once
  operation: increment
  expected:
    - count: "0"
    - $run: increment
    - count: "1"
```

**Construction inputs** a setup needs are declared as ordinary operation
inputs and supplied in the case's `inputs:`; the harness routes each value to
the setup or the call by name:

```yaml
operations:
  increment:
    inputs: { initial: i32 }   # routed to the setup that builds the counter
    outputs: [count]
cases:
  - name: start_at_10
    operation: increment
    inputs: { initial: 10 }
    expected:
      - count: "10"
      - $run: increment
      - count: "11"
```

**Disambiguation with `fills`** — when an operation has more than one
parameter of the setup's output type, or more than one setup produces that
type, each setup pins its target parameter:

```rust
#[spec_setup("transfer", fills = "source")]
fn make_source() -> Account { Account { balance: 100 } }

#[spec_setup("transfer", fills = "target")]
fn make_target() -> Account { Account { balance: 0 } }
```

```yaml
- name: transfer_between_accounts
  operation: transfer
  inputs: { amount: 50 }
  expected:
    - source.balance: "100"
    - target.balance: "0"
    - $run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

Multiple `#[spec_setup(..., fills = ...)]` attributes may be stacked on one
function to build several same-typed parameters. When such parameters need
distinct construction inputs, declare each as a flat `<param>_<fills>`
operation input — e.g. for `make_box(start)` filling `left` and `right`,
declare `inputs: { start_left: i32, start_right: i32 }` and write
`inputs: { start_left: 10, start_right: 5 }` in the case.

### `inputs`

A map of parameter values. Mock response tables go in `inputs` keyed by the
mock's name:

```yaml
- name: find_user_1
  operation: get_user
  inputs:
    id: "user_1"
    db:
      "user_1": "Alice"
  expected:
    - $run: get_user
    - db.request: "user_1"
    - db.response: "Alice"
    - $result: "Alice"
```

### `expected` — subsequence matching

Every entry is one of:

- `{<name>: <value-or-matcher>}` — matches an `Event` with that name.
- `{$run: <operation>}` — matches a `Run` for that operation.
- `{$unordered: [ ... ]}` — matches a group of event assertions in any order.
- `{$anywhere: [ ... ]}` — matches assertions anywhere in the full trace.

The harness validates that every event name asserted in `expected` is
declared in the operation's `outputs` list. Asserting on an undeclared
event name produces an error.

Matching rules:
- Every expected entry must appear in the actual trace stream, **in order**.
- **Gaps are allowed** — extra events may appear between matches.
- **Trailing events are allowed** — the actual stream can be longer.
- Out-of-order expectations fail.

```yaml
expected:
  - count: "0"
  - $run: increment
  - count: "1"
```

#### `$unordered`

Use `$unordered` when several events may appear in any order relative to
each other but still belong at one point in the sequence:

```yaml
expected:
  - $run: withdraw
  - $unordered:
      - balance: "50"
      - transaction_count: "1"
```

Canonical fixture: `unordered_fields.spec.yaml`.

#### `$anywhere`

Use `$anywhere` when an assertion should match somewhere in the trace
regardless of where it occurs relative to the other expected entries:

```yaml
expected:
  - $run: increment_twice
  - count: "2"
  - $anywhere:
      - count: "0"
      - count: "1"
```

Canonical fixture: `anywhere_event.spec.yaml`.

### Structured values and assertion operators

Observed values are structured `Value`s, not flat strings. A `Value` is one of:

- `String`
- `Integer`
- `Float`
- `Bool`
- `List`
- `Map`
- `Set`

Collections emit as single structured events, not flattened per element.

You can assert exact values directly:

```yaml
expected:
  - structural_properties: ["ID", "Name", "Email"]
```

Or use operators:

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

#### Complete operator catalog

Operators are **MongoDB-aligned**. The full set the matcher parses
(`rust/crates/specgate-harness/src/spec.rs`, `parse_single_op`):

| Operator | Argument | Meaning |
|----------|----------|---------|
| `$eq` | value | Explicit deep equality |
| `$ne` | value | Value inequality — not equal to the given value |
| `$gt` | number | Greater than |
| `$gte` | number | Greater than or equal |
| `$lt` | number | Less than |
| `$lte` | number | Less than or equal |
| `$size` | integer | Collection (or string) length equals |
| `$contains` | value or matcher | Collection contains one matching element |
| `$containsAll` | list | Contains every listed value (order-independent) |
| `$excludes` | list | Contains none of the listed values |
| `$match` | map | Partial object match — asserts listed keys, ignores others |
| `$exists` | bool | Field is present (`true`) / absent (`false`) |
| `$any` | value or matcher | At least one element matches |
| `$every` | value or matcher | Every element matches |
| `$type` | string | Value has the given runtime type name |
| `$matches` | string | Regex match (string values only) |
| `$not` | operator expr | Negates **another operator expression** |

Several operators may be combined in one mapping (implicitly AND-ed):
`temperature: { $gt: 60, $lt: 100 }`.

**`$type` accepts** the runtime type names: `string`, `int`, `float`, `bool`,
`list`, `map`, `set`.

**`$ne` vs `$not`** — following MongoDB semantics, `$not` negates an *operator
expression* and never takes a bare value. Use `$ne` for value inequality
(`{ $ne: 100 }`); use `$not` to invert another operator
(`{ $not: { $gt: 5 } }`). Passing a bare value to `$not` is a parse error.

`$contains`, `$any`, and `$every` accept either a literal value or a nested
matcher (`readings: { $every: { $gt: 60 } }`).

#### Trace-position assertions and event keys

Alongside value operators, the `expected:` list uses these top-level keys
(see the harness spec header, `specs/specgate.harness.spec.yaml:7-12`):

| Key | Meaning |
|-----|---------|
| `$run` | Matches a `Run` for the named operation |
| `$unordered` | A group of assertions that may match in any order at one point |
| `$anywhere` | Assertions that match anywhere in the trace, position irrelevant |

Auto-generated event keys addressable from `expected:` are `$result`,
`$outcome`, `$error`, and `$fault` (see "Auto-generated harness fields" above).

Operators may appear at any depth inside a structured value — a plain map that
contains a nested operator is treated as an implicit `$match`, so you can mix
literal fields and matchers without an explicit `$match` wrapper:

```yaml
expected:
  - $result:
      Error:
        reason:
          $matches: "source failed to compile:[\\s\\S]*error"
```

Here `$result` is asserted as a partial object whose `Error.reason` field must
match the regex; other fields (if any) are matched literally. Nesting applies to
maps only — for matchers over list elements, use `$any`, `$every`, or
`$contains`.

Canonical fixtures: `operators.spec.yaml`, `scalar_operators.spec.yaml`,
`structured_output.spec.yaml`, `structured_map.spec.yaml`,
`structured_set.spec.yaml`, `nested_structured.spec.yaml`.

### Multi-step cases

Use `steps:` when a case invokes multiple operations against the same
constructed state (the setup that builds the receiver runs once and is shared
across the steps):

```yaml
- name: increment_then_decrement
  steps:
    - operation: increment
    - operation: decrement
  expected:
    - count: "0"
    - $run: increment
    - count: "1"
    - $run: decrement
    - count: "0"
```

Per-step `expected:` is optional — allows precise per-step assertions:

```yaml
steps:
  - operation: increment
    expected:
      - count: "1"
  - operation: decrement
    expected:
      - count: "0"
```

Case-level `expected:` covers the whole sequence. If both per-step and
case-level expected are provided, both are validated.

### Results, errors, panics, and optionals

Operations returning `Result<T, E>` use auto-generated spec names:

```yaml
# Ok path
expected:
  - $outcome: "Ok"
  - $result: "5"

# Error path
expected:
  - $outcome: "Error"
  - $error: "division by zero"

# Panic
expected:
  - $outcome: "Unrecoverable"
  - $error: "attempt to divide by zero"
```

Operations returning `Option<T>`:

```yaml
# Some path
expected:
  - $outcome: "Some"
  - $result: "1"

# None path
expected:
  - $outcome: "None"
```

## Spec boundary rule

**One spec = one state boundary.** Operations that share state belong in
the same spec; operations with independent state belong in separate specs.
Specs share **types** (not state) via `depends_on:`.

`depends_on` lists the other components (spec names) whose types this spec
references. A component is declared in code with `spec_component!("name")`,
and a spec's `name` **is** its component. See
[annotations.md](annotations.md#spec_component--the-component-axis) for how
components are declared and how per-item overrides work.

