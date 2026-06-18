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
| `name` | yes | Dotted component name, e.g. `fixture.stateless_add` |
| `binding` | no | **Path** (string) to a binding YAML file, relative to this spec file |
| `cases` | yes | List of test cases (≥1; some fixtures intentionally have `cases: []` to test loader behaviour) |
| `depends_on` | no | List of other spec names this spec depends on for shared types |

There are **no** top-level `inputs` / `outcome` / `outputs` /
`state` / `init` / `operations` / `invariants` fields in the current
fixture format. Outcomes are asserted through `expected:` entries on
each case (e.g. `divide.outcome: "Ok"`), not through a top-level
declaration. (The `core.*` self-specs and the harness spec retain some
of these top-level fields for legacy reasons; they are scheduled for
removal.)

## `binding`

A string path. The harness reads the file at that path to learn the
language and where the package under test lives.

```yaml
# fixture.stateless_add.spec.yaml
name: fixture.stateless_add
binding: binding.yaml   # sibling of this spec file
```

```yaml
# binding.yaml
language: rust
targets:
  default:
    package_root: ..
```

See `docs/knowledge/bindings.md` for binding file shape.

## `cases`

```yaml
cases:
  - name: add_2_3              # snake_case, unique within the file
    desc: Adding 2 + 3 returns 5
    operation: add             # name from a #[spec_operation("add")] in source
    inputs: { a: 2, b: 3 }     # passed to the operation by name
    expected:
      - add.result: "5"        # one Event match
```

| Case field | Required | Description |
|------------|----------|-------------|
| `name` | yes | Snake_case identifier, unique within the file |
| `desc` | yes | Human-readable description |
| `operation` | for single-step cases | Operation name (from a `#[spec_operation(…)]`) |
| `steps` | for multi-step cases | Ordered list of `{operation: <name>}` entries |
| `setup` | no | Setup function name (string) or `{alias: setup_fn}` map for multi-setup cases |
| `inputs` | no | Values bound to setup and operation parameters by name |
| `expected` | yes | List of single-entry maps, matched as a subsequence of the trace stream |

`operation` and `steps` are mutually exclusive on a single case.

### `setup`

A case can name a `#[spec_setup("…")]` function:

```yaml
- name: increment_once
  setup: make_counter
  operation: increment
  expected:
    - count: "0"
    - run: increment
    - count: "1"
```

Multi-setup form — aliases map to setup function names, and the aliases
become the operation's parameter names and the prefixes used in
`#[spec_event]` trace names:

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

### `inputs`

A flat map of parameter values. The harness binds each entry to the
matching parameter on the setup or operation. Mock response tables go in
`inputs` too, keyed by the mock's name:

```yaml
- name: find_user_1
  setup: make_service
  operation: get_user
  inputs:
    id: "user_1"
    db:                      # name of a #[spec_mock("db")]
      "user_1": "Alice"      # input → mocked response
  expected:
    - run: get_user
    - db.request: "user_1"
    - db.response: "Alice"
    - get_user.result: "Alice"
```

See `mock_field.spec.yaml`, `mock_multi_response.spec.yaml`,
`mock_not_found.spec.yaml`.

### `expected` — list of single-entry maps

Every entry is one of:

- `{<name>: <value>}` — matches an `Event` with that `name` and stringified `value`.
- `{run: <operation>}` — matches a `Run` for that operation.

Values are compared as strings; quoting them in YAML avoids surprises
with bare numbers and booleans (`"0"`, `"true"`).

```yaml
expected:
  - count: "0"          # Event { name: "count", value: "0" }
  - run: increment       # Run   { operation: "increment" }
  - count: "1"
```

#### Subsequence matching semantics

The `expected:` list is matched as an **in-order subsequence** of the
actual trace stream:

- Every expected entry must appear in the actual trace.
- Order is preserved.
- **Gaps are allowed** — extra events may appear between matches.
- **Trailing events are allowed** — the actual stream can be longer.

So a spec can assert as much or as little as it cares about. The
fixture `subsequence_with_gaps.spec.yaml` only asserts the first and
last value of a counter that increments twice, skipping the intermediate
value:

```yaml
- name: double_increment
  setup: make_counter
  operation: increment_twice
  expected:
    - count: "0"
    - count: "2"          # the intermediate count: "1" is intentionally skipped
```

Out-of-order expectations fail
(`subsequence_wrong_order.spec.yaml`); missing expectations fail
(`mismatch_missing_event.spec.yaml`); extra events in the actual stream
do not.

### Multi-step cases

Use `steps:` instead of `operation:` when a single case invokes more
than one operation against the same setup:

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

`expected:` is still one flat subsequence covering the whole case — there
is no per-step `expected:` and no `assert_state` field. See
`multi_step.spec.yaml`, `mismatch_second_step.spec.yaml`.

### Results, errors, and panics

Operations returning `Result<T, E>` use trace names by convention
(`<operation>.outcome`, `<operation>.result`, `<operation>.error`):

```yaml
# result_ok.spec.yaml
- name: divide_10_by_2
  operation: divide
  inputs: { a: 10, b: 2 }
  expected:
    - divide.outcome: "Ok"
    - divide.result: "5"
```

```yaml
# result_err.spec.yaml
- name: divide_by_zero
  operation: divide
  inputs: { a: 10, b: 0 }
  expected:
    - divide.outcome: "Error"
    - divide.error: "division by zero"
```

Panics are asserted via a `<operation>.error` event whose value is the
panic message (`unrecoverable.spec.yaml`).

## YAML tips

- Bare numbers / booleans are stringified by the matcher; quoting them
  explicitly (`"0"`, `"true"`) keeps the YAML and the trace equal.
- `# yaml-language-server: $schema=…/spec-schema.json` enables editor
  validation (VS Code Red Hat YAML, JetBrains IDEs).
- `[ ]` inline-flow sequences and `{ }` inline-flow mappings are
  equivalent to indented block form.

## Spec boundary rule

**One spec = one state boundary.** Operations that share state belong
in the same spec; operations with independent state belong in separate
specs. No spec composition or include mechanism exists. Specs share
**types** (not state) via `depends_on:`.
