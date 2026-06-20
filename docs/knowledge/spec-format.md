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

- `$result` — return value
- `$outcome` — result / option / panic outcome
- `$error` — error payload or panic message

User-authored capture names stay bare (`count`, `shape.radius`,
`db.request`).

### Operation kinds

| Kind | Description |
|------|-------------|
| *(default)* | Regular annotated operation (discovered via `#[spec_operation]`) |
| `setup` | Factory function that creates state objects |
| `command` | Shell command — exit 0 = pass |

```yaml
operations:
  make_counter:
    kind: setup
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

The implementation must use an async operation entry point. Canonical
fixture: `async_fetch.spec.yaml`.

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
| `setup` | no | Setup operation name (string) or `{alias: setup_name}` map |
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

### `setup`

A case can name a setup operation:

```yaml
- name: increment_once
  setup: make_counter
  operation: increment
  expected:
    - count: "0"
    - $run: increment
    - count: "1"
```

Multi-setup form — aliases map to setup names:

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
    - $run: transfer
    - source.balance: "50"
    - target.balance: "50"
```

### `inputs`

A flat map of parameter values. Mock response tables go in `inputs`
keyed by the mock's name:

```yaml
- name: find_user_1
  setup: make_service
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

Observed values are structured `Value`s, not flat strings. The matcher
supports:

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

| Operator | Meaning |
|----------|---------|
| `$eq` | Explicit equality |
| `$size` | Collection size |
| `$contains` | Contains one value |
| `$containsAll` | Contains all listed values |
| `$excludes` | Contains none of the listed values |
| `$match` | Partial object / map match |
| `$exists` | Field is present |
| `$any` | At least one element matches |
| `$every` | Every element matches |
| `$type` | Value has the given type |
| `$matches` | Regex match |
| `$not` | Negated nested matcher |
| `$gt`, `$gte`, `$lt`, `$lte` | Numeric comparison |

Canonical fixtures: `operators.spec.yaml`, `scalar_operators.spec.yaml`,
`structured_output.spec.yaml`, `structured_map.spec.yaml`,
`structured_set.spec.yaml`, `nested_structured.spec.yaml`.

### Multi-step cases

Use `steps:` when a case invokes multiple operations against the same
setup:

```yaml
- name: increment_then_decrement
  setup: make_counter
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

