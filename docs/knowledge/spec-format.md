# Spec file format

Spec files are YAML, validated by `spec-schema.json`. One file per component or
logical group of operations.

**File convention**: `specs/<name>.spec.yaml`

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Dotted component name, e.g. `core.validate` |
| `binding` | no | Binding file identifier — resolves to `bindings/<binding>.yaml`. Optional for language-agnostic specs; can be specified at run time. |
| `target` | yes | Named target within the binding file |
| `inputs` | no | Named inputs with type/source/desc |
| `types` | no | Named type definitions (oneof or record) |
| `outcome` | yes | Outcome variants or single type |
| `outputs` | yes | Per-outcome output fields |
| `cases` | yes | Test cases (≥1) |

## Type declarations

Types can be inline or named. Use named types for variants (`oneof`) or reuse.

```yaml
# Named type with variants
types:
  Shape:
    oneof:
      Circle: { radius: float }
      Rectangle: { width: float, height: float }

# Named record type
types:
  Point:
    fields:
      x: float
      y: float
```

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

## YAML quirks

- `List<T>` with angle brackets does not need quoting
- `[...]` is YAML flow sequence — quote if used as a string value
- `{ }` is YAML flow mapping — both inline and block indent are equivalent
- Add `# yaml-language-server: $schema=../spec-schema.json` at the top for editor support
