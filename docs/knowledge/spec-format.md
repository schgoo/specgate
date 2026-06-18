# Spec file format

Spec files are YAML, validated by `spec-schema.json`. One spec file per
component (or logical group of operations that share state).

**File convention**: `<name>.spec.yaml` (e.g.
`test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml`).

The canonical examples of every supported pattern live under
`test/rust/crates/specgate-fixtures/specs/`. When this doc and a fixture
disagree, the fixture is the source of truth.

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `spec_version` | yes | Schema version string, currently `"0.3.0"` |
| `name` | yes | Dotted component name, e.g. `fixture.stateless_add` |
| `binding` | no | Path (string) or list of paths to binding YAML files |
| `operations` | yes | Named operations, each declaring its own inputs/outputs/outcome |
| `cases` | yes | List of test cases |
| `types` | no | Named type definitions shared across operations |
| `depends_on` | no | List of other spec names this spec depends on |

## `spec_version`

Required. The harness checks this to determine which spec format to
parse. Current version is `"0.3.0"`.

```yaml
spec_version: "0.3.0"
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

## `operations`

Each operation declares its inputs and outputs. Operations are keyed
by name:

```yaml
operations:
  add:
    inputs: { a: i32, b: i32 }
    outputs: [add.result]
```

### Outputs

Outputs is a list of event names the operation can produce. Each item
is either a bare string (simple event) or a map with type/enum info:

```yaml
# Simple — just event names
outputs: [count, balance]

# With types
outputs:
  - add.result: i32
  - count: i32

# With enum variants and associated data
outputs:
  - outcome:
      oneof:
        Complete:
          results: List<CaseResult>
        Error:
          reason: string
```

The harness validates that `expected` in a case only asserts on events
declared in the operation's outputs.

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
      - outcome:
          oneof:
            Complete: {}
            Error: {}
```

## `cases`

### Runnable cases

```yaml
cases:
  - name: add_2_3              # snake_case, unique within the file
    desc: Adding 2 + 3 returns 5
    operation: add             # must match a key in operations block
    inputs: { a: 2, b: 3 }
    expected:
      - add.result: "5"
```

| Case field | Required | Description |
|------------|----------|-------------|
| `name` | yes | Snake_case identifier, unique within the file |
| `desc` | yes | Human-readable description |
| `operation` | for single-step | Operation name (must match operations block) |
| `steps` | for multi-step | Ordered list of `{operation, inputs?, expected?}` |
| `setup` | no | Setup operation name (string) or `{alias: setup_name}` map |
| `inputs` | no | Values bound to operation parameters by name |
| `expected` | yes | Expected trace assertions (see below) |

`operation` and `steps` are mutually exclusive on a single case.

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
    - run: increment
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
    - run: transfer
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
    - run: get_user
    - db.request: "user_1"
    - db.response: "Alice"
    - get_user.result: "Alice"
```

### `expected` — subsequence matching

Every entry is one of:

- `{<name>: <value>}` — matches an `Event` with that name and stringified value.
- `{run: <operation>}` — matches a `Run` for that operation.

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
  - count: "0"          # Event { name: "count", value: "0" }
  - run: increment       # Run   { operation: "increment" }
  - count: "1"
```

### Multi-step cases

Use `steps:` when a case invokes multiple operations against the same setup:

```yaml
- name: increment_then_decrement
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

Operations returning `Result<T, E>` use trace names by convention:

```yaml
# Ok path
expected:
  - divide.outcome: "Ok"
  - divide.result: "5"

# Error path
expected:
  - divide.outcome: "Error"
  - divide.error: "division by zero"

# Panic
expected:
  - divide.outcome: "Unrecoverable"
  - divide.error: "attempt to divide by zero"
```

Operations returning `Option<T>`:

```yaml
# Some path
expected:
  - find.outcome: "Some"
  - find.value: "1"

# None path
expected:
  - find.outcome: "None"
```

## Spec boundary rule

**One spec = one state boundary.** Operations that share state belong
in the same spec; operations with independent state belong in separate
specs. Specs share **types** (not state) via `depends_on:`.
