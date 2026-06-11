# Spec file format

Spec files are YAML, validated by `spec-schema.json`. One file per component or
logical group of operations.

**File convention**: `specs/<name>.spec.yaml`

## Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | yes | Dotted component name, e.g. `core.validate` |
| `binding` | yes | Binding file identifier — resolves to `bindings/<binding>.yaml` |
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
  Annotation:
    oneof:
      SpecOperation: { operation: string, kind: string }
      SpecSetup: { operation: string, name: string, symbol: string }

# Named record type
types:
  TypeInfo:
    fields:
      name: string
      is_abstract: bool
```

Bare keys (without `fields:` wrapper) are also valid for record types:

```yaml
types:
  TypeInfo:
    name: string
    is_abstract: bool
```

**Types are suggestions, not rules.** They describe what fields must be available,
not how the implementation structures data internally. A Rust enum, a struct, a
trait object — any representation works as long as test cases can construct inputs
and assert outputs using the declared fields.

## Inputs

```yaml
inputs:
  annotations:
    type: List<Annotation>
  source:
    type: string
    source: fixture-file
    desc: Rust source code to compile
```

- `type` is required
- `source` is optional — tells the harness where the input comes from if not a function argument
- `desc` is optional human-readable description

## Outcomes and outputs

```yaml
outcome:
  oneof: [Valid, Invalid]

outputs:
  when Valid:
    warnings: List<ValidationWarning>
  when Invalid:
    errors: List<ValidationError>
    warnings: List<ValidationWarning>
```

Outcome can also be a single string: `outcome: Ok`

## Test cases

```yaml
cases:
  - name: snake_case_name
    desc: Human-readable description
    inputs:
      annotations:
        - SpecOperation: { operation: op_a, kind: Stateless }
    expected:
      outcome: Valid
      warnings: []
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
