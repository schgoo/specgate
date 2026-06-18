# Validation rules

Two kinds of validation happen before a case is executed:

1. **Schema validation** — does the YAML conform to `spec-schema.json`?
2. **Annotation lookup** — do the `setup:` and `operation:` names
   referenced by the case resolve to annotated functions in the source?

## Schema validation

`spec-schema.json` (draft-07) covers the YAML shape:

- `name` and `cases` are required at the top level.
- `binding` is a string (path).
- Each case requires `name` and `desc`; may have `operation`, `setup`,
  `inputs`, `expected`, `steps`, `postconditions`.
- `expected` is an array of single-entry maps (Event matches or `{run:
  …}` entries).

Schema errors are reported as `Invalid` with a `reason`.

## Annotation lookup errors

The fixtures intentionally include cases that exercise these failure
modes:

| Fixture | What it asserts |
|---------|-----------------|
| `missing_setup.spec.yaml` | Referencing a `setup:` name that has no `#[spec_setup]` in the source under test fails the case. |
| `missing_operation.spec.yaml` | Referencing an `operation:` name with no `#[spec_operation]` fails the case. |
| `bad_binding.spec.yaml` | A binding path that does not resolve to a valid binding file fails. |
| `bad_yaml.spec.yaml` | A YAML parse error on the spec file fails. |
| `no_cases.spec.yaml` | A spec with `cases: []` is rejected (or returns an empty result, depending on loader). |
| `compile_error.spec.yaml` | A source file that fails to compile fails the case. |

## Subsequence-match outcomes

A case that loads and runs successfully still fails if the actual trace
stream does not contain `expected:` as a subsequence. The failing
fixtures exercise each shape of mismatch:

| Fixture | What it asserts |
|---------|-----------------|
| `mismatch_missing_event.spec.yaml` | Expected entry never appears in the actual trace. |
| `mismatch_wrong_field.spec.yaml` | Expected event name doesn't match any actual event. |
| `mismatch_second_step.spec.yaml` | The second step's expected slice is absent. |
| `subsequence_wrong_order.spec.yaml` | Two expected entries appear, but in the wrong order. |
| `statemachine_counter_wrong.spec.yaml` | Expected value doesn't match the actual mutation. |

Each is marked `pass` in the harness spec because the harness is
expected to report the case as `fail` — these are negative tests of the
matcher.
