# SpecGate Consistency Review

_Date: 2026-06-18 (UTC)_

This audit compares the schemas, specs, bindings, fixture sources, harness
spec, knowledge docs, and the mechanism-proof test against each other for
internal consistency. No source files were modified.

## Summary

| # | Category | Issues |
|---|----------|--------|
| 1 | Schema ↔ Specs | 3 |
| 2 | Spec ↔ Fixture source | 2 |
| 3 | Binding ↔ Spec | 0 |
| 4 | Harness spec ↔ Fixture specs | 0 |
| 5 | Knowledge docs ↔ Schema | 4 |
| 6 | Fixture source consistency | 0 |
| 7 | Mechanism proof test | 0 |

---

## 1. Schema ↔ Specs

`spec-schema.json` requires every spec document to have
`["name", "spec_version", "operations", "cases"]` (line 6) and declares
`expected` on a runnable case as `"type": "object"` (lines 132–135).

Real (non-negative) specs with violations:

- `specs/core.spec_document.spec.yaml:1` — file lacks the required
  `spec_version` field and the required `operations` block; cases on lines
  84/101/120 reference `operation: validate` which is not defined under
  any `operations:` mapping in the file.
- `specs/core.binding_document.spec.yaml:1` — file lacks the required
  `spec_version` field and the required `operations` block; cases starting
  at line 41 reference `operation: validate` with no `operations:` block.
- `spec-schema.json:132` vs all fixture/runnable cases — the schema declares
  `expected` as `type: object`, but every runnable fixture case uses a
  YAML *list* of trace assertions (e.g.
  `test/rust/crates/specgate-fixtures/specs/stateless_add.spec.yaml:19`,
  `test/rust/crates/specgate-fixtures/specs/multi_step.spec.yaml:21`,
  and 26 other real fixture specs). Either the schema needs `expected`
  to accept `oneOf: [object, array]`, or every spec is wrong; the schema
  is the documented source of truth and is therefore inconsistent with
  observed usage. (Note: `specs/specgate.harness.spec.yaml:101` itself
  uses `expected:` as an object with nested `outcome`/`results`/`traces`
  fields, which matches the schema — so two incompatible shapes are in
  active use.)

Intentional negative fixtures (not issues, classified per task):

- `bad_binding.spec.yaml`, `bad_yaml.spec.yaml`, `compile_error.spec.yaml`,
  `mismatch_missing_event.spec.yaml`, `mismatch_second_step.spec.yaml`,
  `mismatch_wrong_field.spec.yaml`, `missing_operation.spec.yaml`,
  `missing_setup.spec.yaml`, `mock_not_found.spec.yaml`,
  `no_cases.spec.yaml`, `statemachine_counter_wrong.spec.yaml`,
  `subsequence_wrong_order.spec.yaml`, `unrecoverable.spec.yaml` — these
  exist to drive negative test cases in the harness spec and are
  expected to deviate from the schema in controlled ways.

## 2. Spec ↔ Fixture source

Real fixture specs were paired by stem with `test/rust/crates/specgate-fixtures/src/<stem>.rs`.
For each spec, every operation/setup name in the `operations:` block was
checked for a corresponding `#[spec_operation("…")]`,
`#[spec_setup("…")]`, or `#[derive(SpecEvent)]` annotation in the matching
source file (and vice versa).

Pairing gaps (real specs with no same-stem source file):

- `test/rust/crates/specgate-fixtures/specs/multi_field_capture_reordered.spec.yaml:1` —
  no `multi_field_capture_reordered.rs` exists; operations
  (`make_account`, `withdraw`) are satisfied by `multi_field_capture.rs`
  but the same-stem convention documented elsewhere is broken here.
- `test/rust/crates/specgate-fixtures/specs/subsequence_with_gaps.spec.yaml:1` —
  no `subsequence_with_gaps.rs` exists; operations
  (`make_counter`, `increment_twice`) are provided by `multi_mutation.rs`
  but the same-stem convention is broken.

For all other real specs (`stateless_add`, `statemachine_counter`,
`multi_field_capture`, `checkpoint_inline`, `multi_mutation`,
`nested_operations`, `multi_case`, `setup_with_params`, `multi_setup`,
`multi_step`, `mock_field`, `mock_multi_response`, `result_ok`,
`result_err`, `void_operation`, `readonly_operation`), every declared
operation/setup has a matching annotation in the same-stem source file
and vice versa. No missing or stray operations.

Negative-fixture stems that intentionally lack a matching source or
have intentionally inconsistent annotations (`compile_error`,
`missing_operation`, `missing_setup`, `mock_not_found`, `bad_*`,
`mismatch_*`, `no_cases`, `*_wrong*`, `unrecoverable`) are not flagged.

## 3. Binding ↔ Spec

- `bindings/rust.yaml:9-27` declares `language: rust` and the targets
  `run_spec`, `validate_spec`, `mechanism_proof`, `check_fixtures`.
  All three targets named in the audit task (`run_spec`,
  `mechanism_proof`, `check_fixtures`) match operation names actually
  used in `specs/specgate.harness.spec.yaml` cases (lines 64, 73, 98, 117,
  136, 158, 177, 197, …).
- `validate_spec` is declared in the binding (line 16) but not used as
  an operation in `specgate.harness.spec.yaml`; this is permitted by
  `binding-schema.json` (extra targets are allowed) and is therefore
  not a finding.
- `test/rust/crates/specgate-fixtures/specs/binding.yaml:1-5` — well-formed:
  `language: rust`, single target `default` with required `package_root: ..`.
- `binding-schema.json` permits both `function` and `command` as
  alternative dispatch fields on a target, which is what `bindings/rust.yaml`
  uses (`run_spec`/`validate_spec` use `function`; `mechanism_proof`/
  `check_fixtures` use `command`). The structure is permitted by the
  schema.

No issues found.

## 4. Harness spec ↔ Fixture specs

All 31 distinct `spec: …` references in `specs/specgate.harness.spec.yaml`
(lines 100, 118, 137, 159, 178, 199, 228, 249, 276, 293, 320, 341, 366,
384, 401, 418, 432, 451, 468, 486, 503, 522, 539, 563, 581, 604, 613,
622, 631, 640, 649) point to existing files under
`test/rust/crates/specgate-fixtures/specs/`. Cross-checked the harness
case `expected.outcome` for negative-fixture cases:

- `bad_yaml`, `bad_binding`, `missing_setup`, `missing_operation`,
  `compile_error`, `no_cases` cases (lines 600–652) all set
  `outcome: Error` to match their negative-fixture nature.
- `statemachine_counter_wrong`, `mismatch_*`, `subsequence_wrong_order`
  cases set `status: fail` on the inner result, consistent with the
  intent of the corresponding fixture spec.
- Positive cases (`stateless_add`, `statemachine_counter`,
  `multi_field_capture`, `checkpoint_inline`, etc.) all expect
  `outcome: Complete` with `status: pass`, consistent with the fixtures.

No issues found.

## 5. Knowledge docs ↔ Schema

- `docs/knowledge/spec-format.md:99-100` — the example case sets
  `expected:` directly to a *list* (`- add.result: "5"`), but
  `spec-schema.json:132` declares `expected` as `type: object`. The doc
  is consistent with all fixture specs but inconsistent with the schema;
  the same mismatch is the substance of finding 1c above. Either the
  doc or the schema is wrong.
- `docs/knowledge/spec-format.md:110` — the case-field table marks
  `expected` as required, but `spec-schema.json:106` lists only
  `["name", "desc"]` as required for a runnable case. Doc disagrees
  with schema.
- `docs/knowledge/annotations.md:73` — the reference-fixtures table
  labels `checkpoint_inline.rs` as the example for "`spec_event!()`
  inline", but earlier in the same file (line 29) the documented macro
  is `spec_event_record!` and `checkpoint_inline.rs:7` actually uses
  `spec_event_record!`. The "`spec_event!()`" wording is stale.
- `docs/knowledge/spec-format.md:98-100` (and the matching fixture-style
  examples on lines 144-148, 159-165, 200-205, 218-223, 244-258) and
  `docs/knowledge/annotations.md:13-15` — both describe `expected:` as a
  list of trace entries under each case, which contradicts the
  schema's `type: object` declaration; the `traces:` sub-key shape used
  by the harness spec itself (`specs/specgate.harness.spec.yaml:108`,
  `129`, `148`, etc.) is *not* documented in either knowledge doc.

Confirmed correct (no issues for these specific points):

- `spec-format.md:17, 25-32` documents `spec_version: "0.3.0"`.
- `spec-format.md:34-47` documents `binding:` accepting both string and
  list, matching `spec-schema.json:17-26`.
- `spec-format.md:52-86` documents the `operations:` block.
- `spec-format.md:114-134` documents narrative cases with `kind: narrative`,
  `desc`, and `verify`, matching `spec-schema.json:79-102`.
- `spec-format.md:208-238` documents per-step `expected:`, matching
  `spec-schema.json:144-161`.
- `annotations.md:28` correctly states that `#[spec_event]` requires
  `#[derive(SpecEvent)]` on the struct.

## 6. Fixture source consistency

Surveyed all 21 files under `test/rust/crates/specgate-fixtures/src/`:

- No file uses the legacy `spec_event!` macro; only
  `checkpoint_inline.rs:7` uses a recording macro and it correctly uses
  `spec_event_record!`.
- Every `#[spec_event]` field appears inside a struct that carries
  `#[derive(SpecEvent)]` (verified for `missing_operation.rs`,
  `missing_setup.rs`, `multi_field_capture.rs`, `multi_mutation.rs`,
  `multi_setup.rs`, `multi_step.rs`, `nested_operations.rs`,
  `readonly_operation.rs`, `setup_with_params.rs`, `statemachine_counter.rs`,
  `void_operation.rs`).
- All public-API factory functions, structs, and methods carry `pub`
  on real fixtures (e.g. `stateless_add.rs:5`, `statemachine_counter.rs:5`,
  `multi_setup.rs:5,10,21`, `mock_field.rs:5,9,16,22`, etc.).
- The only file lacking `pub` on a top-level item is
  `compile_error.rs:5` (`fn broken() -> i32`). This file is an
  intentional negative fixture (filename matches the `compile_error`
  pattern, and it is deliberately excluded from `lib.rs`'s `pub mod`
  list so it does not get wired into the crate). Classified as
  negative fixture, not a finding.

No issues found.

## 7. Mechanism proof test

`rust/crates/specgate-harness/tests/mechanism_proof.rs`:

- Lines 21-24 import from `specgate_fixtures` (`statemachine_counter`,
  `stateless_add`, `multi_field_capture`, `checkpoint_inline`); line 29
  imports `take_traces` from `specgate_annotations`. Both crate
  dependencies match the audit requirement.
- Lines 1-19 contain the file header. Line 3 explicitly states
  "DO NOT EDIT THIS FILE", and lines 4-6 explain that these tests are
  hand-written and "must not be modified by any automated tool or
  agent." The "do not modify by agents" notice is **present**.
- The four tests (lines 31, 40, 57, 78) each call a real fixture
  function and then assert on `take_traces()`, consistent with the
  proof-of-mechanism intent.

No issues found.
