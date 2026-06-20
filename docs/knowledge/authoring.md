# Authoring a Spec

This guide walks you through writing a `.spec.yaml` file from scratch.
For the exhaustive field reference, see [`spec-format.md`](spec-format.md).

## Minimal spec

```yaml
spec_version: "0.4.0"
name: my.component
binding: binding.yaml

operations:
  greet:
    inputs: { name: string }
    outputs: [$result]

cases:
  - name: greet_world
    desc: Greeting "World" returns "Hello, World!"
    operation: greet
    inputs: { name: "World" }
    expected:
      - $result: "Hello, World!"
```

Every spec needs:
- `spec_version` — current schema version (see `spec-schema.json`)
- `name` — dotted component name
- `binding` — path to a binding file (connects spec to source code)
- `operations` — what the component can do
- `cases` — concrete test scenarios

## Declaring operations

Each operation declares its inputs and outputs:

```yaml
operations:
  add:
    inputs: { a: i32, b: i32 }
    outputs: [$result]

  create_user:
    inputs:
      name: string
      email: string
    outputs:
      - user_id: string
      - $result: string
```

- `$result` — the return value of the function
- Named outputs (like `user_id`) — values emitted via `spec_trace!` or `#[spec_event]`
- Use `kind: setup` for factory functions that construct state

## Writing test cases

A case exercises one operation and asserts on the trace:

```yaml
cases:
  - name: add_positive
    desc: Adding two positive numbers
    operation: add
    inputs: { a: 2, b: 3 }
    expected:
      - $result: "5"
```

### What to assert on

- `$result` — the operation's return value
- `$run: <operation>` — that an operation was invoked
- Any named output — values your code emits via annotations
- Operators for structured matching:

```yaml
expected:
  - $result:
      $gt: 0
      $lt: 100
  - items:
      $size: 3
      $contains: "foo"
```

### Multi-step cases

For stateful components, use `setup` and `steps`:

```yaml
cases:
  - name: increment_twice
    desc: Counter goes from 0 to 2
    setup: make_counter
    steps:
      - operation: increment
      - operation: increment
    expected:
      - count: "0"
      - $run: increment
      - count: "1"
      - $run: increment
      - count: "2"
```

## Defining types

For complex inputs/outputs, declare types in the `types` block:

```yaml
types:
  Point:
    fields:
      x: i32
      y: i32
  Shape:
    oneof:
      Circle:
        radius: i32
      Rectangle:
        width: i32
        height: i32
      Point: {}

operations:
  sum_points:
    inputs:
      points:
        type: list
        items: Point
    outputs:
      - $result: Point
```

Types map to Rust structs/enums with `#[derive(SpecEvent)]`.

## Property test cases

For invariants that should hold across random inputs:

```yaml
cases:
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

Generator types: `i32[min, max]`, `f64[min, max]`, `bool`, `string[min_len, max_len]`,
`string[min_len, max_len, pattern: "regex"]`,
`oneof["a", "b"]`, `list[type, len: min..max]`, `set[type, size: min..max]`,
`map[key, val, size: min..max]`, `optional[type]`.

## Narrative cases

For constraints that can't be machine-checked:

```yaml
cases:
  - name: no_network_in_tests
    kind: narrative
    desc: >
      Generated test runners must not make network calls.
      All external dependencies are mocked.
    verify:
      - Check generated code has no reqwest/hyper imports
```

## Level and source

For specs derived from a standard (e.g., OData, HTTP RFCs):

```yaml
cases:
  - name: must_reject_invalid_uri
    desc: Invalid URI returns 400
    level: must
    source:
      spec: RFC 3986
      section: "3.1"
      assertion_ids: [RFC3986-URI-1]
    operation: parse_uri
    inputs: { uri: "not a uri" }
    expected:
      - $result: "Error"
```

`level` affects behavior when the annotation is missing:
- `must` — error (default)
- `should` — warning
- `may` — skip

## Binding file

The spec references a binding that points to your code:

```yaml
# binding.yaml
language: rust
targets:
  default:
    package_root: ../my-crate
```

See [`bindings.md`](bindings.md) for target configuration.

## Validation

Before implementing, validate your spec:

```bash
specgate validate specs/
```

This catches schema errors, undefined operations, missing inputs, and more.

## Next steps

- [`fixtures.md`](fixtures.md) — topical index of all fixture specs (canonical examples)
- [`annotations.md`](annotations.md) — how to annotate your Rust code
- [`spec-format.md`](spec-format.md) — exhaustive field reference
- [`rust.md`](rust.md) — Rust-specific conventions
- [`greenfield.md`](greenfield.md) — implementing a new spec from scratch
