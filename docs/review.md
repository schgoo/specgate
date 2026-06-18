# SpecGate Review

This review validates the consistency of the SpecGate repository across three
dimensions: (1) the harness spec and its fixture specs/sources, (2) the
project documentation, and (3) the JSON Schema and the self-describing
`core.*spec.yaml` files. It was produced by three parallel reviewers and
consolidated into a single report.

The repository is mid-migration from an older, richer "Kind taxonomy /
state-machine / outcome+outputs" model to a simpler **Event/Run trace +
subsequence matching** model. The README and the fixtures already use the
new model. The JSON Schema, the two self-specs, most of the design /
knowledge docs, and a handful of fixture and harness entries still describe
or partially encode the old one. That drift is the single biggest source of
findings below.

## 1. Summary of findings

### Healthy / consistent
- **README.md** describes the new model accurately (spec format example,
  annotations table, binding file example). One minor terminology slip
  ("subset" vs "subsequence").
- **`specgate.harness.spec.yaml`** correctly enumerates 30 cases against the
  fixtures, and 24 of them cross-check cleanly (fixture `expected` matches
  the harness `expected`, source annotations exist, traces are plausible).
- **`bindings/rust.yaml`** and `binding-schema.json` are largely consistent
  with each other and with the active fixtures.
- The simplified Event/Run model is internally consistent in the fixtures
  themselves: every fixture source uses only `#[spec_operation("name")]`,
  `#[spec_setup]`, `#[spec_event]` (on fields), `spec_event!(...)`, and
  `#[spec_mock]` — matching the README annotation table.

### Significant drift / bugs

1. **`spec-schema.json` is critically out of date.** It models the old
   `{name,target}` object binding, object-shaped `expected`, and the
   state-machine subformat with `additionalProperties:false` on
   `testCase`. Every fixture spec and `specgate.harness.spec.yaml` would
   fail validation against this schema today.

2. **`specs/core.spec_document.spec.yaml` actively teaches the wrong format.**
   `SpecCase` is missing `operation`/`setup`, declares `expected` as a map,
   and all 16 embedded `yaml: |` examples are in the OLD format.

3. **`docs/design.md` is ~80% legacy** (Kind taxonomy, ITF/Quint two-wave
   architecture, contexts/dependencies, `assert_state`, decomposed
   primitives). The "simplified two-variant trace model" appears as an
   island in an otherwise outdated 900+ line document.

4. **`docs/knowledge/spec-format.md` and `docs/knowledge/annotations.md`**
   describe rules that no longer apply: `binding` as `{name,target}`,
   object-shaped `expected`, `kind` required on operations, `spec_capture`
   / `spec_checkpoint!()` annotations that aren't used.

5. **High-severity correctness bug in `unrecoverable_panic`**: harness and
   fixture both expect panic text `"attempted to divide by zero"`, but
   Rust's actual runtime panic is `"attempt to divide by zero"`. The case
   is marked `pass` but would deterministically fail.

6. **`specgate.harness.spec.yaml:21`** uses string-path `binding:` with no
   `target:` sibling; the current Rust `validate_spec_document` (legacy
   path) would error out loading it. Schema also rejects it.

7. **Three fixture specs use map-form `expected:` instead of the list form**
   used by every other fixture (`missing_setup`, `missing_operation`,
   `compile_error`). Tolerated only because those cases error before
   evaluation.

8. **`specs/core.binding_document.spec.yaml`** references a non-existent
   binding target (`test-types`) and authors its cases with `input:`
   (singular) + case-level `outcome:`/`outputs:` — the meta-schema it points
   at (`spec-schema.json`, `additionalProperties:false`) would reject this.

9. **`docs/issues.md`** has multiple issues that look effectively resolved
   under the new model (ISS-005, ISS-007, ISS-010 partial, ISS-011 partial,
   ISS-013 partial) and at least one (ISS-008) that references files no
   longer in the tree.

## 2. Issues table

Severity legend: **critical** (blocks correctness or validity), **high**
(broken behavior or major misalignment), **medium** (incorrect/stale but
non-blocking), **low** (cosmetic / nit), **info** (verified or
context-only).

### 2.1 Spec / fixture / harness consistency

| file | line | issue | severity |
|---|---|---|---|
| `specs/specgate.harness.spec.yaml` | 367 | `unrecoverable_panic` expects `divide.error: "attempted to divide by zero"`. Rust's actual panic message is `"attempt to divide by zero"`. The case is marked `pass` but would deterministically fail. | high |
| `test/rust/crates/specgate-fixtures/specs/unrecoverable.spec.yaml` | 8 | Same wrong panic text (`"attempted to divide by zero"`). | high |
| `test/rust/crates/specgate-fixtures/src/` | n/a | No `result_err.rs` file exists and `lib.rs` has no `pub mod result_err;`, but `specs/result_err.spec.yaml` references operation `divide`. The op is only defined in `result_ok.rs`, so two specs implicitly share one source — diverging from the otherwise 1:1 spec↔source convention. | medium |
| `test/rust/crates/specgate-fixtures/src/lib.rs` | 1‑19 | `compile_error` module is intentionally commented out (OK) but `result_err` simply isn't declared anywhere — easy to mistake for an oversight rather than intentional reuse. | low |
| `test/rust/crates/specgate-fixtures/specs/compile_error.spec.yaml` | 6‑7 | `expected:` is a map (`broken.result: "42"`) instead of the list‑of‑maps form used by every other fixture spec. | low |
| `test/rust/crates/specgate-fixtures/specs/missing_setup.spec.yaml` | 7‑8 | Same — `expected: count: "1"` is a map, not a list. | low |
| `test/rust/crates/specgate-fixtures/specs/missing_operation.spec.yaml` | 7‑8 | Same — `expected: count: "1"` is a map, not a list. | low |
| `test/rust/crates/specgate-fixtures/specs/bad_binding.spec.yaml` | 1‑12 | Uses an entirely different (harness-style) schema: `target: test`, `outcome: Complete`, `outputs.when Complete.report: RunReport`. Not the fixture-case schema used by the other 27 specs. | low |
| `test/rust/crates/specgate-fixtures/specs/bad_yaml.spec.yaml` | 1‑10 | Same — harness-style schema, plus an `extra: [unterminated` line to force YAML parse failure. Intentional malformedness OK; mixed schema style is the inconsistency. | low |
| `test/rust/crates/specgate-fixtures/specs/no_cases.spec.yaml` | 1‑9 | Same — harness-style schema with `cases: []`. | low |
| `specs/specgate.harness.spec.yaml` | 177‑193 | `setup_with_input_params`: `desc` claims "Setup function parameters are traced", but the asserted traces only contain `count: "10"` then `count: "11"` — only the field event, not the input parameter `initial`. | low |
| `specs/specgate.harness.spec.yaml` | 307‑319, 221‑236, 387‑398, 340‑354, 356‑367, 403‑414 | These cases omit the `traces` field on the CaseResult, but the `CaseResult` type lists `traces: List<TraceEvent>` without a default. Either mark optional or fill every case. | low |
| `test/rust/crates/specgate-fixtures/src/void_operation.rs` | 14 | `log(&mut self, msg: &str)` ignores `msg`. Harmless — consider `_msg`. | info |
| `test/rust/crates/specgate-fixtures/src/mock_field.rs`, `mock_multi_response.rs` | 9‑12 | `RealDb::find(&self, id: &str)` ignores `id`. Mocks intercept, but unused-param warning. | info |
| `test/rust/crates/specgate-fixtures/specs/mock_not_found.spec.yaml` | 13 | Expected `db.error: "no mock response for input 'user_99'"`. Exact-string assertion with no shared constant — brittle. | info |
| `test/rust/crates/specgate-fixtures/specs/missing_setup.spec.yaml` | 4 | Case requests `setup: make_counter`; `missing_setup.rs` has none, BUT `make_counter` IS defined in five other fixture sources. Case only produces the expected error if the harness scopes annotation lookup per-source-file. | medium |
| `test/rust/crates/specgate-fixtures/specs/missing_operation.spec.yaml` | 4 | Same scoping concern: `operation: increment` is annotated in multiple other fixture sources. | medium |
| `test/rust/crates/specgate-fixtures/specs/mismatch_wrong_field.spec.yaml` | 4‑6 | Same `make_counter`/`increment` multi-source ambiguity. | info |
| `test/rust/crates/specgate-fixtures/specs/statemachine_counter_wrong.spec.yaml` | 4‑6 | Same multi-source ambiguity. | info |
| `specs/specgate.harness.spec.yaml` | 420‑434 | `mismatch_wrong_value` correctness verified — fail expected, fail produced. | info |
| `specs/specgate.harness.spec.yaml` | 470‑487 | `mismatch_second_step` correctness verified — fail expected, fail produced. | info |
| `specs/specgate.harness.spec.yaml` | 493‑508 | `subsequence_wrong_order` correctness verified — fail expected, fail produced. | info |

### 2.2 Documentation consistency

| file | line | issue | severity |
|---|---|---|---|
| `docs/design.md` | 14‑15 | "Kind = Stateless/StateMachine/Sequence/ErrorMap/Structural" — simplified model no longer has these. Fixtures use `#[spec_operation("name")]` with no `kind`. | critical |
| `docs/design.md` | 48 | The "Traces use a simplified two-variant model" statement is correct, but the surrounding ~900 lines still describe the old model (ITF/Quint, decomposed primitives, contexts/dependencies). The simplified section is an island. | high |
| `docs/design.md` | 90 | "Trace collection ── ITF traces for Quint (optional)" — fixtures use plain Event/Run JSON; no ITF/Quint pipeline exists in code. | high |
| `docs/design.md` | 100‑106, 114‑146 | Three-file-types section claims `binding: rust` resolves to `bindings/<lang>.yaml` and `target:` selects a target. Fixtures use `binding: binding.yaml` (a path); `README.md:98` says explicitly so. Direct contradiction. | critical |
| `docs/design.md` | 121‑138 | The `inputs:`/`outputs:` sub-keys (`file:`, `env:`, `arg:`, `stderr: true`, `command:`/`call:`) under binding targets — no fixture binding file uses any of these. Doesn't match `binding-schema.json` or actual binding files. | high |
| `docs/design.md` | 151‑156 | `binding: rust` + separate top-level `target: build` — used only by the legacy form retained for back-compat. | high |
| `docs/design.md` | 168‑246 | Top-level fields and "Each `cases` entry has" tables describe `inputs`/`expected`/`steps`/`assert_state` with `expected` as an object map. Fixtures use a list of single-key maps and case-level `operation:`/`setup:`. | critical |
| `docs/design.md` | 246 | "expected return value (partial match)" — current model is **subsequence** matching of trace events, not partial-field return matching. | high |
| `docs/design.md` | 263‑311 | State-machine example uses `assert_state` and per-step `expected`. Real state-machine fixtures use a flat list `expected:` and no `assert_state`. | critical |
| `docs/design.md` | 319‑356 | Entire "Quint generation"/ITF/invariant proposer block is aspirational; no `.qnt`, no proposer in `rust/crates/`. | high |
| `docs/design.md` | 382‑414 | "Two-wave architecture for state machines" — aspirational; not in current code. | high |
| `docs/design.md` | 418‑454 | Annotation system table is OK but surrounding "Multi-place annotations"/"role = Setup\|Checkpoint\|State\|Mock" framing describes a `kind`/`role` system fixtures do not use. | high |
| `docs/design.md` | 462‑476 | "Construction resolution" with auto-discovered constructors + parameter bubbling — not present in fixtures (setup params come from case-level `inputs:`). | medium |
| `docs/design.md` | 484‑544 | Large C# `[SpecOperation("findByKey", Kind = Sequence)]` / `[SpecCheckpoint]` / `[SpecSetup(..., Name=...)]` example — uses Kind taxonomy that no longer exists. References `csharp-harness.md` (file does not exist). | high |
| `docs/design.md` | 547‑566 | "Kind determines extraction strategy" / "Completeness is statically checkable" — Kinds not present in code. | high |
| `docs/design.md` | 569‑636 | "Execution model: inputs, contexts, dependencies" — none of contexts/dependencies are in any fixture or fixture source. | high |
| `docs/design.md` | 640‑697 | "Schema: decomposed primitives" / "Input generation" / `[SpecGenerator]`, `[Spec.Constraint]` — none of this exists. | high |
| `docs/design.md` | 701‑730 | "Claim validation" / "Scorecard" with `(trials, threshold)` — not present; current harness is per-case pass/fail. | medium |
| `docs/design.md` | 734‑775 | "Iterative annotation guidance" with mutation testing / proxy signals — not implemented. | medium |
| `docs/design.md` | 944 | "(line 564-577)" cross-reference is stale (spec-type-system table is at 911‑921). | low |
| `docs/design.md` | 974‑975 | "Q6 (annotation syntax): C# syntax defined in `csharp-harness.md`" — that file does not exist. | medium |
| `docs/knowledge/annotations.md` | 18‑24 | Trace-emitted column for `#[spec_setup]` says "`Event { name, value }` for each argument". Plausible but not visibly demonstrated; README repeats it. Consistent at least. | info |
| `docs/knowledge/annotations.md` | 28 | "Operation requires both the operation name and `kind`" — every `#[spec_operation("…")]` in fixtures is name-only. Rule is wrong for the current simplified model. | critical |
| `docs/knowledge/annotations.md` | 30‑31 | "`spec_event` on a field…/on a method captures the return value after the operation" — fixtures only use `#[spec_event]` on **fields**; no fixture uses it on a method. Method form is undocumented in fixtures. | medium |
| `docs/knowledge/annotations.md` | 46‑55 | "Output capture" section introduces `spec_capture` and `spec_checkpoint!()` — these do not appear in any fixture source (only `#[spec_event]` and `spec_event!()` are used). Internal inconsistency with table at 18‑24. | critical |
| `docs/knowledge/annotations.md` | 56‑58 | "For StateMachine operations, captured fields are recorded **before and after** the operation call. For all other kinds, fields are captured **after** only." — depends on Kind taxonomy that no longer exists. | high |
| `docs/knowledge/spec-format.md` | 10‑23 | Top-level fields table treats `binding` as `{name, target}` resolving to `bindings/<name>.yaml`. Fixtures use `binding: binding.yaml` (path string). README confirms path-based, not name-based. | critical |
| `docs/knowledge/spec-format.md` | 25‑27 | "single-operation vs state machine" exclusivity — active fixture format has neither `inputs`/`outcome`/`outputs` at top level nor `state`/`operations`. Case-level mode is undocumented here. | critical |
| `docs/knowledge/spec-format.md` | 92‑148 | "Outcomes and outputs" with `oneof`/`when Ok:` blocks — no fixture uses this. Outcomes are now asserted via list entries like `divide.outcome: "Ok"`. | critical |
| `docs/knowledge/spec-format.md` | 150‑168 | Test cases section shows `expected:` as an object. Real fixture cases use a LIST of single-key maps (subsequence). Also says `desc` is required — bare fixtures like `bad_binding.spec.yaml` lack `desc`. | critical |
| `docs/knowledge/spec-format.md` | 170‑200 | "Per-case binding target" with `binding: { name, target }` and `binding.target` override — not used by any fixture. | high |
| `docs/knowledge/spec-format.md` | 209‑246 | "State machine specs" with top-level `state`/`init`/`operations`/`invariants` — fixtures express multi-step via case-level `setup:` + `steps:`; no top-level `state`/`init`/`invariants`. | critical |
| `docs/knowledge/spec-format.md` | 248‑273 | Multi-step cases describe per-step `expected` and `assert_state`. Fixture `multi_step.spec.yaml` uses ONE flat list `expected:` after `steps:`; `assert_state` is not used anywhere. | critical |
| `docs/knowledge/spec-format.md` | 296‑322 | Postconditions — present in `SpecCase` struct but unused by any fixture. Plausible but unused. | info |
| `docs/knowledge/spec-format.md` | 30‑69 | Type declarations / `causes` keyword — used by `core.spec_document.spec.yaml` meta-spec but presented here as the normal authoring path. | medium |
| `README.md` | 27‑55 | Spec format example matches fixtures. Correct. | info |
| `README.md` | 58‑66 | Annotations table matches fixtures. Correct. | info |
| `README.md` | 84 | Says "subset matching"; actual model is **subsequence** (order-preserving). Minor terminology drift. | medium |
| `README.md` | 89‑98 | Binding file format example matches `bindings/rust.yaml` and fixture binding. Correct. | info |
| `README.md` | 104 | "specgate.harness.spec.yaml (35 test cases)" — actual count appears to be ~30. Verify. | low |
| `README.md` | 108‑117 | Project structure references only `rust/crates/specgate-types/`. Fixtures `use specgate_annotations::*;`, so `specgate-annotations` and `specgate-runtime` exist on disk but are missing from the listing. | medium |
| `README.md` | 122‑125 | "Milestone 1: Annotations + Runtime ✦ *in progress*" with unchecked boxes — fixtures already use the full annotation API. Status is stale. | medium |
| `docs/issues.md` | 9 | ISS-001 "Per-case build configurations" — still legitimately open. | info |
| `docs/issues.md` | 13 | ISS-005 "Claims syntax" — simplified model has no notion of "claims"; the issue is effectively moot. | medium |
| `docs/issues.md` | 15 | ISS-007 "Property-based testing" — already noted "subsumed by two-wave architecture", but the two-wave architecture itself is gone. Should be closed (superseded). | medium |
| `docs/issues.md` | 16 | ISS-008 "Command target exit code bug" — references `render_command_case` in `generator.rs` ~line 495. No such file exists in current `rust/crates/specgate-types/src/`. Likely obsolete. | medium |
| `docs/issues.md` | 18 | ISS-010 "Spec YAML schema validation in Rust" — `validate_spec_document` exists in `spec_document.rs`. Should be Partially-Implemented. | medium |
| `docs/issues.md` | 19 | ISS-011 "Spec dependency DAG" — `core.spec_document.spec.yaml` exists; `depends_on:` is in schema and struct. Should be Closed/Partial. | medium |
| `docs/issues.md` | 20 | ISS-012 "Generated test file quality" — depends on legacy generator design; likely moot under spec-as-code direction (ISS-015). | low |
| `docs/issues.md` | 21 | ISS-013 "Annotation spec — deferred runtime cases" — Ok/Err/panic fixtures now exist; Result<T,E> gap closed. Should be partially closed. | medium |
| `docs/issues.md` | 22 | ISS-014 "Enforce trust boundary on validation artifacts" — already Closed. | info |
| `docs/issues.md` | 167‑171 | ISS-009 references `specgate-rust-backend` — not visible under `rust/crates/`. May reference removed code. | medium |
| `docs/issues.md` | 219‑238 | ISS-012 describes `specgate_generated.rs` issues; under ISS-015 spec-as-code direction this is likely obsolete. | low |
| `docs/issues.md` | 277‑302 | ISS-015 — decision 1 (trace as sole source of truth) is realized; decision 2 (spec-as-code Rust library) — no `specgate-spec` crate. Partly realized. | info |
| `docs/issues.md` | 306‑307 | "Version 1.3 / Last Updated 2026-06-16" — no entry records the spec-format simplification. A new issue/log entry should capture that docs and schema are drifting. | high |

### 2.3 Schema / self-spec consistency

| file | line | issue | severity |
|---|---|---|---|
| `spec-schema.json` | 14‑24 (`binding`) | Schema only allows `binding` as `{name,target}` object or array thereof. Real specs use a string path (`specgate.harness.spec.yaml:21` `binding: bindings/rust.yaml`; every fixture `binding: binding.yaml`). README documents it as a path. Schema needs a string variant. | critical |
| `spec-schema.json` | 6 (`required: [name, cases]`) + 123 (`cases.minItems: 1`) | Rejects `no_cases.spec.yaml:9` which legitimately has `cases: []`. | high |
| `spec-schema.json` | 7 (top-level `additionalProperties: false`) | Real specs use top-level `target:` alongside string `binding:` (`bad_binding.spec.yaml:4`, `no_cases.spec.yaml:4`, `bad_yaml.spec.yaml:3`); Rust deserializer (`spec_document.rs:62`, 127‑133) supports this. Schema rejects `target` at top level. | critical |
| `spec-schema.json` | 25‑32, 40‑60, 62‑82 (`inputs`/`outcome`/`outputs`) | Modeled as first-class top-level keys, but fixture format has dropped them. Only `core.*spec.yaml` and `specgate.harness.spec.yaml` still use them. | medium |
| `spec-schema.json` | 83‑109 (`state`/`init`/`operations`/`invariants`) | State-machine subformat does not appear in any current fixture or in README. Dead surface area. | medium |
| `spec-schema.json` | 261‑326 (`testCase`, `additionalProperties:false` at 264) | Forbids any property other than `name, desc, binding, inputs, expected, steps, postconditions`. Fixtures use **`operation`** (every fixture) and **`setup`** (e.g. `statemachine_counter.spec.yaml:6`, `multi_setup.spec.yaml:6‑8`). `setup` can be string or map. | critical |
| `spec-schema.json` | 283‑292 (`testCase.expected`) | Schema declares `expected` as `type: object` with an `outcome` property. Fixtures use a list of single-key maps (e.g. `stateless_add.spec.yaml:8‑9`). | critical |
| `spec-schema.json` | 244‑254 (`testStep.expected`) | Modeled as object, but in new format steps are just `{operation: name}` (see `multi_step.spec.yaml:7‑9`). | high |
| `spec-schema.json` | 231‑260 (`testStep`) | Fixture steps never use `inputs`, `expected`, or `assert_state`; only `operation`. | low |
| `spec-schema.json` | 12 (`name` pattern requires a dot) | Single-token names like `bad_binding`, `no_cases`, `bad_yaml` fail this pattern. | low |
| `spec-schema.json` | 277 (`testCase.binding: $ref bindingEntry`) | Same locked `{name,target}` shape propagates to case-level binding override. | low |
| `spec-schema.json` | 301‑325 (`postconditions`) | No current spec uses them. | info |
| `specs/core.spec_document.spec.yaml` | 39 (`binding: BindingEntry or List<BindingEntry>`) | Only documents object/list forms. Misses string-path form and the legacy `binding: name` + sibling `target:` form actually accepted by the Rust parser and used by every fixture and `specgate.harness.spec.yaml:21`. | critical |
| `specs/core.spec_document.spec.yaml` | 75‑86 (`SpecCase`) | Missing `operation` and `setup` fields. All fixture cases use `operation:`; most use `setup:`. Two most-used case-level fields are absent. | critical |
| `specs/core.spec_document.spec.yaml` | 81 (`expected: Map<string, Value>`) | Declares case `expected` as a map. Fixtures and README use it as a list of `{name: value}` entries. Self-spec contradicts the documented format. | critical |
| `specs/core.spec_document.spec.yaml` | 92‑96 (`TestStep`) | Declares step with `operation, inputs, expected, assert_state`. Real fixtures use only `operation` per step. | medium |
| `specs/core.spec_document.spec.yaml` | 44‑48 (`state/init/operations/invariants`) | Documents a state-machine subformat that is no longer present in any fixture or in README. | medium |
| `specs/core.spec_document.spec.yaml` | 105‑605 (test cases) | All 16 embedded `yaml: \|` examples use the OLD format. Self-spec's own examples teach the wrong format. | high |
| `specs/core.spec_document.spec.yaml` | 16‑19 | Target `validate-spec` matches `bindings/rust.yaml:8`. ✓ | info |
| `specs/specgate.harness.spec.yaml` | 21 (`binding: bindings/rust.yaml`) | Uses string-path form. Violates current `spec-schema.json`. No sibling `target:`, so Rust parser's legacy path would error `missing field 'target'`. Spec cannot currently be loaded by `validate_spec_document`. | high |
| `specs/specgate.harness.spec.yaml` | 49‑578 (cases) | Case-level `expected:` uses OLD object shape (`outcome`, `results`, `reason`); nested `results[*].expected:` is a list of `{name: value}` matching new format. Schema cannot describe that inner shape at all. | medium |
| `test/rust/.../specs/stateless_add.spec.yaml` | 2 / 6 / 8‑9 | String binding, case-level `operation:`, list-form `expected`. Triple violation of current schema. | critical |
| `test/rust/.../specs/statemachine_counter.spec.yaml` | 2, 6, 7, 8‑11 | Same triple violation. | critical |
| `test/rust/.../specs/multi_setup.spec.yaml` | 6‑8 | `setup:` is a **map** `{source: make_source, target: make_target}` — must be `oneOf: [string, map<string,string>]`. Currently unmodeled. | critical |
| `test/rust/.../specs/multi_step.spec.yaml` | 7‑15 | `steps: - operation: ...` is OK against `testStep`; but case-level list `expected:` (10‑15) still rejected. | high |
| `test/rust/.../specs/mismatch_second_step.spec.yaml` | 7‑11 | Steps with just `operation`, plus case-level list `expected`. | high |
| `test/rust/.../specs/bad_binding.spec.yaml` | 1, 3‑4 | Has `$schema=../../spec-schema.json` directive but: name has no dot, `binding: nonexistent` is a string, `target: test` at top level, case `expected: {outcome: Complete}` (map). Multiple violations vs schema. | high |
| `test/rust/.../specs/no_cases.spec.yaml` | 1, 3‑4, 9 | Same as above, plus `cases: []` violates `minItems: 1`. | high |
| `test/rust/.../specs/bad_yaml.spec.yaml` | 9 | Intentionally malformed YAML — schema is N/A. | info |
| `test/rust/.../specs/mock_field.spec.yaml` | 8‑11 | `inputs.db:` is a map of mock responses; schema permits. ✓ | info |
| `bindings/rust.yaml` | 5‑11 | `language: rust` and `targets.validate-spec` conform to `binding-schema.json`. ✓ | info |
| `bindings/unknown_lang.yaml` | 1 | `language: unknown_lang` violates `binding-schema.json:12` enum `["rust", "csharp"]`. Intentional fixture. | info |
| `bindings/bad_schema.yaml` | 1‑4 | Missing required `language`, unknown keys. Intentional bad-schema fixture. | info |
| `specs/core.binding_document.spec.yaml` | 18‑19 | `binding: - name: rust, target: test-types` — but `bindings/rust.yaml` only declares target `validate-spec`. Broken binding target reference. | high |
| `specs/core.binding_document.spec.yaml` | 110‑122 (`target_with_both_command_and_function`) | Self-spec asserts `Invalid`, but `binding-schema.json` does not encode that constraint. Validator needs extra logic the JSON Schema doesn't express. | medium |
| `specs/core.binding_document.spec.yaml` | 46‑158 (case bodies) | Cases use `input:` (singular) and place `outcome:`/`outputs:` at the case level. Per `spec-schema.json` `testCase.additionalProperties:false` (line 264), only `inputs` (plural) is allowed. Self-spec authored in a shape its own meta-schema rejects. | high |
| `specs/core.binding_document.spec.yaml` | 58, 72, 88, 97, 105, 119, 131, 147, 158 | Each case has top-level `outcome:` and `outputs:` siblings of `input:` — same `additionalProperties:false` problem. | high |

## 3. Recommendations

### Highest priority

1. **Fix the `unrecoverable_panic` panic-message text.** In both
   `specs/specgate.harness.spec.yaml:367` and
   `test/rust/crates/specgate-fixtures/specs/unrecoverable.spec.yaml:8`,
   change `"attempted to divide by zero"` to `"attempt to divide by zero"`
   so the assertion matches Rust's actual runtime panic. Without this fix
   the harness case fails on first run.

2. **Update `spec-schema.json` to match the new fixture format.**
   - Allow `binding` to be a string (file path) in addition to the
     `{name,target}` object form; add top-level `target:` as a sibling
     string for the legacy form.
   - Add `operation` and `setup` to `testCase`; allow `setup` as either a
     string or a `Map<string,string>` (alias→setup_fn).
   - Allow `testCase.expected` to be either an array of single-key
     `{name: value}` maps (the new subsequence form, including `run: <op>`
     entries) or the existing object form (legacy).
   - Drop `additionalProperties: false` on `testCase` (or enumerate every
     field actually used).
   - Relax `cases.minItems` to `0` so `no_cases.spec.yaml` passes.
   - Relax the `name` pattern to allow single-token names like
     `bad_binding`, or rename those fixtures.
   - Either remove or clearly gate the state-machine subformat
     (`state`/`init`/`operations`/`invariants`) behind a `oneOf` until any
     fixture uses it.

3. **Rewrite `specs/core.spec_document.spec.yaml` to reflect the new
   format.**
   - Document `binding: string` (path) — keep `BindingEntry` as legacy.
   - Add `operation: string` and `setup: string or Map<string,string>` to
     `SpecCase`.
   - Change `expected: Map<string, Value>` to
     `expected: List<Map<string, string>>` with subsequence semantics.
   - Move state-machine fields to a legacy types section.
   - Replace all 16 embedded `yaml: |` examples with new-format examples.

4. **Rewrite `docs/design.md` to reflect the simplified model.** Keep the
   requirements list (highlight req 11‑13), keep the "Spec boundaries = state
   boundaries" framing, and replace the Kind taxonomy / Quint / ITF /
   two-wave / contexts/dependencies / Setup-Checkpoint-State-Mock-role
   content with the actual Event/Run + subsequence model. Move legacy
   material to `appendix-historical.md` or a clearly-marked "Vision /
   Future Work" section.

5. **Rewrite `docs/knowledge/spec-format.md` to match the fixtures.**
   - `binding:` is a relative file path string.
   - Cases have `operation:`, optional `setup:` (string or `{alias: name}`),
     optional `inputs:`, and `expected:` is a list of single-key maps
     matched as a subsequence of the trace stream.
   - There is no top-level `outcome`/`outputs`/`state`/`init`/`operations`/
     `invariants` in current fixture specs; outcomes are asserted via trace
     events.
   - Mark `assert_state` / per-step `expected` / top-level state-machine
     forms as superseded or "future".

6. **Fix `docs/knowledge/annotations.md`:**
   - Remove the "kind required" rule (line 20 column header + line 28).
   - Remove the entire "Output capture" section that introduces
     `spec_capture` / `spec_checkpoint!()` (they aren't used), or replace
     with: field capture uses `#[spec_event]`; inline capture uses
     `spec_event!("name", &expr)`.
   - Remove the StateMachine-only before/after distinction (lines 56‑58) —
     capture timing is uniform for `#[spec_event]` fields.

### Medium priority

7. **`specs/specgate.harness.spec.yaml:21`** — either add a `target:` field
   (legacy form) or wait for the schema/parser to accept the bare
   string-path binding form. Today this spec cannot be loaded by
   `validate_spec_document`.

8. **Normalize the three fixture specs that use map-form `expected:`** —
   convert `compile_error`, `missing_setup`, `missing_operation` to the
   list-of-maps form used by the other 25 fixture specs.

9. **Resolve the `result_err` source ambiguity.** Either add a dedicated
   `test/rust/crates/specgate-fixtures/src/result_err.rs` (declared in
   `lib.rs`) with its own `divide` annotation, or document in
   `result_err.spec.yaml` that it intentionally shares `result_ok.rs`.

10. **Clarify error-case fixture spec schema.** `bad_binding`, `bad_yaml`,
    and `no_cases` follow the harness/operational spec shape. Either
    comment that this is intentional (testing parser/loader behavior) or
    migrate them to the fixture-case shape; especially `no_cases.spec.yaml`
    could simply be `name: …\nbinding: binding.yaml\ncases: []`.

11. **Mark `traces` optional on `CaseResult`** in
    `specs/specgate.harness.spec.yaml` (lines 43‑47) — or add explicit
    `traces:` blocks to the six cases that omit them
    (`multiple_cases_one_spec`, `mock_input_not_in_table`,
    `result_err_path`, `unrecoverable_panic`, `readonly_operation`,
    `event_order_between_runs`).

12. **`docs/issues.md` re-triage:**
    - ISS-005 (claims syntax): close as obsolete or re-frame.
    - ISS-007 (property-based testing): close as superseded; two-wave
      architecture has been dropped.
    - ISS-008 (command target exit code): verify `generator.rs` /
      `render_command_case` still exist; if not, close.
    - ISS-009 (ohno migration): verify `specgate-rust-backend` still
      exists.
    - ISS-010 (schema validation in Rust): downgrade to "Partially
      Implemented" — `validate_spec_document` exists.
    - ISS-011 (depends_on / shared types spec): close as
      "Closed (partial)" — `depends_on` exists in schema and Rust struct,
      `core.spec_document.spec.yaml` exists.
    - ISS-012, ISS-013 (generated test file quality, deferred runtime
      cases): scope-shift in light of ISS-015 and current fixtures.
    - ISS-015 (generator scope leak): note that the
      trace-as-sole-source-of-truth decision is realized; remaining work is
      the `specgate-spec` Rust library.
    - Add a new ISS-016 (or similar): **"Docs and JSON schema drift —
      re-align with simplified Event/Run trace + subsequence model"** and
      track items 2‑6 above against it.

13. **Binding self-spec fixes:**
    - In `specs/core.binding_document.spec.yaml:19`, change
      `target: test-types` to `target: validate-spec` (or whichever target
      exists in `bindings/rust.yaml`).
    - Rewrite the cases to use `inputs:` (plural) and express
      validity via the new list-form `expected:` instead of case-level
      `outcome:`/`outputs:`.
    - If "target cannot have both `command` and `function`" is a real rule
      (case at 110‑122), encode it in `binding-schema.json` via
      `oneOf`/`not` on `targetDefinition`.

### Cosmetic

14. Update `README.md:84` from "subset" to "subsequence" to match the
    actual matcher semantics.

15. `README.md` — mark Milestone 1 items done where demonstrably done, and
    add `specgate-annotations` and `specgate-runtime` to the project
    structure listing (lines 108‑117).

16. `docs/design.md:944` — fix the stale `(line 564-577)` line reference.

17. `docs/design.md:974‑975` — either add `csharp-harness.md` or remove
    the reference.

18. **Tighten the `setup_with_input_params` description**
    (`specs/specgate.harness.spec.yaml:177‑193`) — either change `desc:` to
    "Setup runs with input parameter" or extend the asserted trace to
    include the input parameter capture event.

19. **Pin the mock-not-found error string.** Add a comment in
    `mock_not_found.spec.yaml` (or a shared constant) noting that the exact
    `"no mock response for input 'user_99'"` text must match the harness
    emitter.

### Cross-cutting

20. **Add CI that runs every `.spec.yaml`** (both `specs/` and
    `test/rust/.../specs/`) through both (a) `spec-schema.json` and (b)
    `validate_spec_document`, so future drift between schema, parser, and
    fixtures is caught immediately. This was the root cause of nearly every
    schema-side finding in this report.
