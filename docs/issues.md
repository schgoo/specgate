# Issue Log: SpecGate

> Decisions, open questions, and deferred work tracked during spec design and implementation.

## Summary

| ID | Title | Status | Date |
|----|-------|--------|------|
| ISS-001 | Per-case build configurations | Open | 2026-06-11 |
| ISS-002 | fundle crate for construction resolution | Open | 2026-06-11 |
| ISS-003 | Perf counters and code coverage in bindings | Open | 2026-06-11 |
| ISS-004 | External test case files | Open | 2026-06-11 |
| ISS-005 | Claims syntax (non-functional and narrative) | Open | 2026-06-11 |
| ISS-006 | Report renderer (report.render) | Open | 2026-06-11 |
| ISS-007 | Property-based testing in specs | Open | 2026-06-12 |
| ISS-008 | Command target exit code bug | Open | 2026-06-12 |
| ISS-009 | ohno migration for error types | Open | 2026-06-12 |
| ISS-010 | Spec YAML schema validation in Rust | Open | 2026-06-12 |
| ISS-011 | Spec dependency DAG and shared types spec | Open | 2026-06-13 |

---

## ISS-001: Per-Case Build Configurations

**Context**: Specs are language-agnostic but may need cases that build with different configurations (features, flags, compile-time options). The binding file controls the build command, but it's one command for all cases.

**Status**: Open — deferred until we have a concrete use case.

**Options**:
- Cases express semantic config dimensions (e.g., `async: true`) that bindings map to language-specific flags
- Binding defines multiple named configs, cases reference one by name
- Per-case command overrides in the binding

**Impact**: Without this, all cases in a spec must share the same build configuration. Not blocking for current specs.

---

## ISS-002: fundle Crate for Construction Resolution

**Context**: The harness resolves a construction graph (work backwards from entry point, find constructors/setups for each type). This is conceptually DI resolution.

**Status**: Open — deferred until the harness is being built.

**Options**:
- Use `fundle` as a conceptual model only
- Use `fundle` as runtime wiring in generated test code
- Roll our own resolution (the algorithm is already specified in the design doc)

**Impact**: Affects generated test code structure. The construction resolution algorithm is already designed — this is about implementation reuse.

---

## ISS-003: Perf Counters and Code Coverage in Bindings

**Context**: The harness spec defines `PerfMetrics` (cpu_cycles, instructions, cache_misses, branch_misses) and `CoverageReport` (per-file line/branch coverage) as output types. The binding schema needs to declare how these are collected.

**Status**: Open — deferred until basic command target execution works end-to-end.

**Options**:
- **Perf**: cachegrind, `perf stat`, or other CPU counter tools
- **Coverage**: llvm-cov, tarpaulin (Rust), dotCover/coverlet (C#)

**Impact**: The binding would optionally specify which tool to use and how to parse its output. Output types are already defined in `harness.run.spec.yaml`.

---

## ISS-004: External Test Case Files

**Context**: Spec files can become very long when they have many test cases. Need a mechanism to split cases across files.

**Status**: Open — deferred until a spec exceeds ~50 cases.

**Options**:
1. YAML `!include` tag (non-standard but common):
   ```yaml
   cases:
     - !include cases/happy_path.yaml
   ```
2. Custom `cases_from` field (explicit):
   ```yaml
   cases_from:
     - cases/harness.run.happy.yaml
     - cases/harness.run.errors.yaml
   ```
3. Directory convention (`cases/<spec-name>/*.yaml`)

**Impact**: Affects spec schema, YAML parsing, and tooling. Need to pick an approach that editors and schema validation can handle.

---

## ISS-005: Claims Syntax (Non-Functional and Narrative)

**Context**: The design doc distinguishes measurable claims (statistical, with trials and threshold) from narrative claims (human-reviewed only). No spec syntax exists for either yet.

**Status**: Open — design separately since it's cross-cutting across all specs.

**Needs**:
- **Non-functional claims**: timing budgets, memory limits, throughput requirements. Each has `(trials, threshold)` config.
- **Narrative claims**: human-reviewed assertions like "error messages should be actionable." Explicitly flagged as not machine-checkable.
- **Claim IDs**: every assertion traceable to a specific spec clause.

**Impact**: Affects the spec schema (new `claims:` section?), the scorecard output, and the harness execution model. Currently specs only have functional test cases — no way to express "p95 latency < 10ms" or "UI feels responsive."

---

## ISS-006: Report Renderer (report.render)

**Context**: The harness (`harness.run`) outputs structured JSON with all case results, perf metrics, and coverage data. A separate tool should render this into a human-readable HTML conformance report.

**Status**: Open — deferred until harness output format is stable.

**Reference**: The DSAPI conformance report (`conformance-report.html`) is the design target — collapsible scenario cards, side-by-side expected/actual comparison, divergence lists, coverage overlay, expand/collapse all.

**Impact**: Needs its own spec (`report.render`). The harness deliberately does NOT generate HTML — separation of data and presentation.

---

## ISS-007: Property-Based Testing in Specs

**Context**: Spec cases are concrete examples (`{a: 2, b: 3} → 5`) that an implementation could hardcode. Property-based tests define universal assertions over value ranges — randomized inputs that can't be gamed.

**Status**: Reframed — subsumed by the two-wave architecture (see design.md § Workflows).

**Original approach**: A `properties` section with explicit `for_all`/`assert` syntax:
```yaml
properties:
  add_commutative:
    for_all: { a: int(-1000, 1000), b: int(-1000, 1000) }
    assert: add(a, b) == add(b, a)
```

**New approach (wave 2)**: Property testing is now automatic, not hand-written:

1. **Wave 1**: Run existing tests with instrumentation → collect ITF traces → propose invariants from observed patterns → user approves
2. **Wave 2**: Generate Quint model from traces + approved invariants → `quint run` random simulation explores novel operation sequences → export ITF traces → replay as proptests

The user never writes property assertions manually. Invariants are inferred from trace analysis (always-contains, never-empty, monotonic growth, implication, bounded, idempotent) and proposed to the user for approval. The approved invariants become the property tests.

**Language mapping** (unchanged):
- Rust: `proptest!` for trace replay
- C#: equivalent property test framework

**What changed**: The `properties` YAML section is no longer needed. Instead, `invariants` holds approved properties, and wave 2 tooling generates the proptests. The three tiers become: cases (concrete, hand-written), invariants (universal, inferred + approved), types/constraints (structural).

**Tooling needed**: `specgate trace`, `specgate propose-invariants`, `specgate quint-gen`, `specgate proptest-gen`.

---

## ISS-008: Command Target Exit Code Bug

**Context**: `render_command_case` in `generator.rs` (~line 495) always emits `assert!(output.status.success())`. This fails for spec cases where the expected outcome is `Error` and the command exits non-zero.

**Status**: Open — known bug. Spec cases `command_target_error_exit` and `command_target_mixed_outcomes` expose this.

**Fix**: Skip the success assertion when `expected.outcome == Error`. Parse stdout/stderr for error JSON regardless of exit code.

**Impact**: Blocks command-target specs from testing error paths. API-target and annotation-target paths are unaffected.

---

## ISS-009: ohno Migration for Error Types

**Context**: `RunError` and `GenerateError` are still plain Rust enums. Per the error model redesign (checkpoint 020), they should migrate to `#[ohno::error]` structs. The `causes` spec keyword maps to individual ohno structs composed with `#[from]` and inspected with `find_source::<T>()`.

**Status**: Open — deferred until harness runs end-to-end.

**Impact**: Affects `specgate-types` (RunError), `specgate-rust-backend` (GenerateError), and all code that matches on these types. Non-breaking for external consumers if done correctly (struct-based errors are additive).

---

## ISS-010: Spec YAML Schema Validation in Rust

**Context**: Spec YAML files are validated by `spec-schema.json` in editors (via `yaml-language-server` directive), but there is no programmatic schema validation in the Rust toolchain. The harness parses YAML with `serde_yaml` but does not validate against the JSON Schema — structural errors (wrong field names, missing required fields, mutually exclusive field violations like `inputs` + `operations`) are caught ad-hoc or not at all.

**Status**: Open.

**Options**:
- `jsonschema` crate — mature, supports draft-07, validates `serde_json::Value` against a schema
- `valico` crate — alternative JSON Schema validator
- Custom validation in the harness parser — less general but no new dependency

**Scope**: Add a `validate_spec_schema(yaml_str) -> Result<(), Vec<SchemaError>>` to `specgate-types` or `specgate-harness`. Call it during `run_spec` before parsing into `SpecDocument`. Also usable as a standalone `specgate validate` CLI command.

**Impact**: Catches spec authoring errors early with actionable messages. Especially important now that specs have two modes (single-operation vs state machine) with mutual exclusivity constraints.

---

## ISS-011: Spec Dependency DAG and Shared Types Spec

**Context**: `harness.core` and `harness.rust` both depend on `SpecDocument` (defined in `specgate-types`), but neither spec declares this dependency. If `SpecDocument` changes shape (e.g., adding `state`/`operations` for state machine support), the system doesn't know to re-validate downstream specs. More broadly, specs that share types across boundaries need a way to declare and enforce those contracts.

**Design decisions made**:

1. **Spec boundary rule is about shared mutable state, not shared types.** Operations sharing state belong in one spec. Operations sharing types (read-only data contracts) stay in separate specs but declare the dependency.

2. **Shared types get their own spec.** A `core.spec_document` (or similar) spec defines the shared types: `SpecDocument`, `SpecCase`, `TestStep`, `BindingFile`, etc. Consumer specs declare `depends_on: [core.spec_document]`.

3. **`spec-schema.json` is maintained separately but tested for consistency.** The types spec is the source of truth for type shapes. A conformance test asserts that `SpecDocument` (Rust struct) can round-trip everything `spec-schema.json` allows, and that the schema rejects anything the types spec doesn't define. The JSON Schema adds validation rules (patterns, mutual exclusivity, minItems) that go beyond type shapes.

4. **`spec-schema.json` should be versioned.** Schema changes are breaking changes for consumers. A version field enables forward/backward compatibility detection.

5. **`depends_on` field in specs.** A list of spec names. When `specgate validate` runs, it builds the DAG and re-validates anything downstream of a changed spec. No special format needed — the contract is the type definitions in the dependency's spec.

**Scope**:
- Add `depends_on` to spec format and `spec-schema.json`
- Write a `core.spec_document` spec defining shared types
- Add version field to `spec-schema.json`
- Add conformance test: `SpecDocument` struct ↔ `spec-schema.json` ↔ types spec
- Update `specgate validate` (future) to build and check the DAG

**Why not merge specs instead**: Following the type-sharing chain merges everything — `harness.rust` shares `SpecDocument` with `harness.core` AND shares `Annotation` with `rust.annotations`. Merging on types collapses all specs into one. The DAG preserves modularity while making contracts explicit.

---

**Version**: 1.0
**Last Updated**: 2026-06-13
