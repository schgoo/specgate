# Copilot Instructions — SpecGate

## Start here

Read `AGENTS.md`, `docs/README.md`, `docs/digests/llm.md`, and the relevant CTSC
contract before planning or changing behavior. Use the observe → plan → act →
verify workflow in `docs/agentic-loop.md`. Do not silently resolve a
human-owned decision listed in `AGENTS.md`.

## Gate

**Always run `just check` from the repository root before committing.**

Useful focused recipes:

```powershell
just build
just test
just clippy
just format-check
just deny
just readme-check
just ctsc-validate
just ctsc-smoke
just ctsc-goldens-check
just ctsc-goldens-update
just package-smoke
just dotnet-build
just dotnet-test
just format-check-cs
```

## CTSC-first workflow

Behavior changes update or add focused CTSC registry, native trace, capture, or
replay tests. Capture behavior at real annotated operation boundaries through
ordinary tests. Do not add a separate behavioral assertion format or flat trace
compatibility path.

`binding-schema.json` is the CTSC target-binding format. Paths are relative to
the binding file. The active CLI surface is:

```text
specgate discover <binding> ...
specgate capture <binding> ...
specgate replay <capture> <candidate-binding> ...
specgate validate <registry|trace|linked|bundle> ...
specgate compare <reference-trace> <candidate-trace> ...
```

## Architecture

1. `specgate-runtime` owns native capture, semantic values, and link-time
   operation/type metadata.
2. `specgate-annotations-macros` instruments operation boundaries and emits
   setup/type metadata; `specgate` is the sole Rust facade.
3. `specgate-discovery` owns strict binding resolution, Rust discovery builds,
   C# compiled-assembly reflection, raw invocation metadata, and deterministic
   semantic normalization/setup folding.
4. `specgate-ctsc` owns registry/trace encoding, bundle digest/link
   verification, replay models, and validators.
5. `specgate-cli` exposes discovery, Rust reference capture, and Rust candidate
   replay.

Async operations remain discoverable, but native capture rejects them before
polling because capture state is thread-local and cannot safely cross `.await`.

## Golden matrix

`test/goldens/ctsc/matrix.json` is hand-authored configuration covering every
annotated fixture source: component rows, negative discovery/build rows, and
replacement rows that point removed spec features at their CTSC-native tests.
Every other file under `test/goldens/ctsc` is generated product output.

- Never hand-edit a generated golden. Run `just ctsc-goldens-update` and review
  the artifact diff.
- `just ctsc-goldens-check` runs inside `just check`. It regenerates into
  repository-local scratch, validates with the native CTSC validators, checks
  cross-language parity, replays the rows marked replayable, and byte-compares
  against the checked-in goldens.
- Adding a fixture component requires a matrix row; adding an annotated source
  without a row fails the gate. The harness also compares the components each
  compiled target declares against the matrix rows, so an extra component in an
  already-covered file fails too.
- A `parity` exception must declare every allowed Rust-vs-C# difference exactly
  (`parity.allowedDifferences`: JSON Pointer plus each language's value); the
  observed recursive diff must equal the declared set.
- Golden capture is strict: any failing enumerated fixture test fails the batch.
  Every discovered operation must also appear in a captured trace, including
  nested calls, unless `captureExclusions` names that exact operation with a
  stable code and reason. `specgate capture` keeps its historical "capture what
  passes" behavior.
- A replay row marked `unsupported` must declare `expectCategory`; only that
  stable planner limitation is accepted, never an unrelated replay failure.
- Every compiler error in a build-negative row must have primary spans
  exclusively in the row's intentional sources. Unspanned or unrelated errors
  fail the gate even when the intended diagnostic is also present.

## Conventions

- Every annotated Rust crate declares `spec_component!("dotted.name")`.
- Public operations use `#[spec_operation("snake_case_name")]`.
- Setup selection is keyed by exact component + operation and must reject
  ambiguity. Running one setup declaration twice in a capture is accepted only
  when both runs record value-identical inputs.
- Capture records the registry's folded surface: `#[spec_setup]` construction
  inputs become the operation's inputs, and setup-filled parameters are omitted.
- Every captured bundle must pass trace, linked, and bundle validation.
- Structured values implement `ToNativeValue`, normally via
  `#[derive(SpecEvent)]`.
- C# annotations are metadata only; preserve raw declaring type, method,
  signature, static/public/async/setup, and exception information.
- Observation declarations and comparison profiles are not implemented; do not
  claim otherwise.
- Crate READMEs are generated from crate-level docs. Edit `lib.rs`, then run
  `just readme`.
- Use `ohno` for new Rust error types.
- Use LF except `.ps1`, `.cmd`, and `.bat`.
