# Validation rules

SpecGate validation happens at two times:

1. **Static validation** — `specgate validate <spec-dir>` checks each
   `.spec.yaml` against the schema and a set of semantic/runnability rules,
   *before* anything is compiled or run. This is the authoring-time quality
   gate (see "Static validation" below).
2. **Runtime validation** — when a case is executed, the harness checks that
   the `setup:`/`operation:` names resolve to annotated functions and that the
   emitted traces match the case's `expected:` (see "Schema validation" and
   the matcher sections below).

## Static validation (`specgate validate`)

`specgate validate <spec-dir>` recursively validates every `*.spec.yaml` under
a directory and is the standard authoring-time gate (run it in CI alongside the
harness). Findings are `error` (must fix) or `warn` (advisory); `--strict`
promotes warnings to errors. Exit code is non-zero if any error is reported.

**Schema / semantic checks (always on):**

| Check | Severity | Fires when |
|-------|----------|-----------|
| `schema` | error | YAML won't parse, top-level isn't a map, or `spec_version` is missing |
| `operation_reference` | error | a case names an operation not declared in `operations:` |
| `input_completeness` | error | a case is missing a declared input, or supplies an undeclared one (a map-valued extra is allowed — treated as a mock table) |
| `name_uniqueness` | error | two cases share a `name` |
| `expected_format` | error | an `expected` entry has more than one key |
| `runnable_expected` | error | a runnable case has neither `expected` nor `steps`-with-expected |
| `dep_consistency` | error | an operation's `depends_on` names an undeclared operation |
| `narrative_misuse` | warn | a `kind: narrative` case has `verify` steps that look machine-testable |

**Runnability checks (always on)** — mirror the hard errors the harness raises,
so authors catch them before a run:

| Check | Severity | Fires when |
|-------|----------|-----------|
| `no_cases` | error | the spec has no cases |
| `binding_present` | error | no `binding:` is declared |
| `binding_resolves` | error | the `binding:` path doesn't resolve/parse |
| `target_exists` | error | a referenced `target:` isn't in the binding |
| `package_root_exists` | error | a used target's `package_root` directory is missing |
| `operation_annotated` | error | a cased operation has no `#[spec_operation]` in the source |
| `setup_wiring` | error | a case's setups can't be wired (missing/ambiguous receiver or `fills`) |
| `source_setup_visibility` | error | a `#[spec_setup]` function isn't `pub` |
| `source_field_visibility` | error | an operation input-type struct field isn't `pub` |

`operation_annotated` and `setup_wiring` use the harness's own scanner/resolver
(`specgate_harness::check_runnable`), so static validation agrees exactly with
an actual run. Command targets (those with a `command:`) are excluded — they run
via a shell command, not annotated source.

Pass `--spec-only` to skip the source-dependent runnability checks
(`package_root_exists`, `operation_annotated`, `setup_wiring`, and the two
visibility checks) when authoring a spec before its implementation exists.

**Assertion-aware checks (only with `--assertions-dir <dir>`, or a resolved
`<spec-dir>/sources/assertions`):**

| Check | Severity | Fires when |
|-------|----------|-----------|
| `assertion_coverage` | error | a `source.assertion_ids` entry references an id not in the assertions dir |
| `level_correctness` | warn | a case's `level` differs from a referenced assertion's level |
| `mixed_level_bundle` | warn | a case bundles both `must` and `may` assertions |
| `negative_coverage` | warn | a negatable assertion is referenced but never by a negative case |

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
