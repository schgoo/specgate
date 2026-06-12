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

**Status**: Open — design after harness runs end-to-end.

**Proposed syntax**:
```yaml
properties:
  add_commutative:
    for_all: { a: int(-1000, 1000), b: int(-1000, 1000) }
    assert: add(a, b) == add(b, a)
  add_identity:
    for_all: { a: int }
    assert: add(a, 0) == a
```

**Language mapping**:
- Rust: `proptest!` or `quickcheck` macros
- C#: `FsCheck` or similar

**Rationale**: The implement-spec skill reads spec cases to understand requirements but doesn't run the harness (treat like ML validation data). Properties strengthen this — even if the skill sees the property definition, it can't hardcode outputs for randomized inputs.

**Needs**: Value range DSL design, spec-schema.json update, spec-format.md update, generator support for both languages.

**Impact**: Adds a third tier to specs: cases (concrete), properties (universal), types/constraints (structural). Significantly increases confidence that implementations are correct, not just case-fitted.

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

**Version**: 1.0
**Last Updated**: 2026-06-12
