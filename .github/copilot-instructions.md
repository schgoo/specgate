# Copilot Instructions — SpecGate

## Build, test, and quality gates

**Always run `just check` from the repo root before committing.**

This is the complete cross-language gate: Rust build/tests/clippy/rustfmt/license
checks, spec validation, extraction golden checks, generated README checks,
ignored self-host/conformance/coverage tests, C# build/tests, and C# formatting
and analyzers. Do not push code that fails it.

Useful narrower recipes:

```powershell
just build
just test
just clippy
just format-check
just validate
just extract-check
just self-host
just conformance-self-host
just cli-self-host
just coverage
just dotnet-build
just dotnet-test
just format-check-cs
```

Run a focused Rust test from `rust/`:

```powershell
cargo test -p specgate-cli count_value_tokens_matches_whole_words_only
cargo test -p specgate-harness --test harness_self_test self_test_stateless_add_produces_real_traces
```

Run a focused C# test from the repository root:

```powershell
dotnet test test\csharp\SpecGateFixtures\Tests\SpecGateFixtures.Tests.csproj --filter "FullyQualifiedName~Add_2_3_EmitsCanonicalTraceJson"
```

The pinned Rust toolchain is in `rust/rust-toolchain.toml`. `just check` also
expects `cargo-deny`, `cargo-doc2readme`, and the LLVM tools component used by
coverage.

## Architecture

SpecGate verifies language implementations by comparing deterministic runtime
traces with `.spec.yaml` expectations:

1. `specgate-types` owns the serde models for specs, bindings, and reports.
2. `specgate-annotations-macros` instruments Rust operations, setups, events,
   and mocks; `specgate-annotations` is its public facade.
3. `specgate-runtime` stores traces/mocks and exposes the link-time operation
   and type registry used by extraction and discovery.
4. `specgate-harness` loads a spec and binding, discovers annotated public
   operations, generates a temporary runner, invokes the real Rust/.NET
   toolchain, collects traces, and subsequence-matches them against expected
   assertions. It also owns coverage and cross-target trace comparison.
5. `specgate-cli` exposes `validate`, `run`, and `extract`; `specgate` is the
   umbrella crate re-exporting annotations and optional harness support.

The top-level `specs/` files self-describe the harness, CLI, and cross-language
conformance ledger. Binding files select language, package root, target, runtime,
and invocation; every binding path is relative to the binding file itself.

Rust fixture crates live in the separate `test/rust` workspace. C# annotations
are compile-time metadata; `SpecGate.Weaver` rewrites built assemblies to call
`SpecGate.Runtime`. Dual-bound fixtures must produce byte-identical canonical
traces across Rust and C#.

## Spec-first implementation

**All implementation changes must go through the spec implementation skill.**

For feature, bug-fix, or behavior changes in Rust or C#:

1. Update or author the relevant `.spec.yaml`; use
   `.github/skills/author-spec.md` for spec authoring.
2. Validate new specs with `specgate validate <dir> --spec-only`.
3. Delegate implementation with: `Follow the implementation skill at
   .github/skills/implement-spec.md`.
4. Review the generated source/tests and verify both direct tests and the
   harness run.

Never write or edit implementation code directly; always delegate to a
subagent. The implementation skill uses `.specgate/snapshots/` hashes to
reconcile changed specs and their transitive dependents.

Direct edits without a subagent are only acceptable for:

- Build configuration (`Cargo.toml`, `.csproj`)
- CI/CD and tooling (`.github/workflows`, scripts)
- Documentation (`docs/`, knowledge files)
- Spec files, schemas, and bindings
- Test fixtures and integration test wiring

## Repository-specific conventions

- The spec, binding, and hand-written source are the implementation inputs.
  Never inspect or copy harness-generated runners, traces, or scratch artifacts
  under `target/specgate-harness/`; that makes verification circular.
- `docs/knowledge/index.md` is the routing page for task-specific guidance.
  Read only the relevant topic files. When documentation and a fixture differ,
  the canonical fixtures under `test/rust/crates/specgate-fixtures/` win.
- Every Rust annotated crate declares its component with
  `spec_component!("dotted.name")`; public operations use
  `#[spec_operation("snake_case_name")]`. Setups, mocks, and language-neutral
  parameter names must be explicit through the corresponding annotations.
- The C# surface intentionally mirrors Rust sum types and trace serialization.
  Preserve `Option<T>`/`Result<T, E>` naming and byte-level ordering; analyzer
  exceptions in `.editorconfig` are deliberate.
- Crate READMEs are generated from crate-level `lib.rs` documentation using
  `rust/crates/README.j2`. Edit the Rust docs, then run `just readme`; do not
  hand-edit generated crate READMEs.
- Extraction output is deterministic. After an intentional emitter change, run
  `just extract-update`, review both spec and sibling binding diffs, then ensure
  `just extract-check` passes.
- Use `ohno` for newly authored Rust error types; do not introduce `thiserror`
  or `anyhow`.
- Use LF for source, docs, specs, schemas, and generated text. `.ps1`, `.cmd`,
  and `.bat` are the explicit CRLF exceptions in `.gitattributes`.
