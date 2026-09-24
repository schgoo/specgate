# Contributing to SpecGate

## Setup

1. Install the pinned Rust toolchain from `rust/rust-toolchain.toml`.
2. Install `just`, `cargo-deny`, and `cargo-doc2readme`.
3. Install Python dependencies from `docs/ctsc/requirements.txt`.
4. Install the .NET 10 SDK.

## CTSC-first development

Behavior changes must update or add focused registry, native trace, capture, or
replay tests. Capture behavior at real operation boundaries through ordinary
tests; do not introduce a parallel assertion language or flat trace sink.

Use `binding-schema.json` for Rust/C# target bindings. The active CLI commands
are `discover`, `capture`, `replay`, `validate`, and `compare`.

## Planning and agent workflow

Start with `AGENTS.md` and `docs/README.md`. Use the observe → plan → act →
verify loop in `docs/agentic-loop.md`: define one bounded vertical slice with
explicit responsibility boundaries, implement it through the existing CTSC
mechanisms, then independently inspect the diff and validation evidence.

Normative CTSC shapes and validation, public CLI shape, capture/replay identity
and stimulus semantics, comparison policy semantics, runtime package identity,
and golden-matrix declaration meaning require an explicit human owner. Record
ratified architectural decisions under `docs/decisions/`.

## Golden matrix

`test/goldens/ctsc/matrix.json` is the reviewable configuration for the whole
fixture corpus. Edit it by hand when you add, remove, or reclassify a fixture:
every annotated fixture source must belong to a row, every discovered component
needs a row, and each row states its language phases, expected outcome, parity
mode, linkage mode, replay mode, and any limitation. Linkage is `linked` for
every row that captures a bundle and `not-applicable` for discovery-only rows;
a bundle that will not link is a defect to fix, not a row to annotate. A
`parity` exception must also list every allowed Rust-vs-C# difference as a JSON
Pointer plus the exact value each language emits; the harness requires the
observed difference set to equal the declared one.

The check runs in both directions: the components each compiled target actually
declares must be exactly the components the matrix rows name, and every
enumerated fixture test must pass under golden capture. Every discovered
operation must be exercised by a captured trace (nested calls count), unless
the row's `captureExclusions` names the exact operation with a stable code and
reason. Unsupported replay rows likewise declare the exact `expectCategory`;
an unrelated planner or discovery error never satisfies them. Build negatives
reject any compiler error without primary spans exclusively in the row's
intentional sources, even when the intended error is also present.

Every other file under `test/goldens/ctsc` is generated. Do not hand-edit them.

```powershell
just ctsc-goldens-update   # regenerate artifacts after an intended change
just ctsc-goldens-check    # regenerate into scratch and byte-compare (part of just check)
```

After an intentional change, run `just ctsc-goldens-update`, review the
artifact diff as carefully as the code diff, then confirm
`just ctsc-goldens-check` passes.

## Gate

Run from the repository root:

```powershell
just check
```

The gate covers Rust build/tests/clippy/fmt/licenses, generated crate READMEs,
the CTSC Python corpus and linked validators, a deterministic capture-to-replay
smoke, the CTSC golden matrix check, and the retained C# build/tests/format/
analyzers.

Run `just package-smoke` for release changes; it packages all six retained crates,
installs the packaged CLI, and exercises registry-dependency
discover/capture/replay.

Use Conventional Commits and keep changes scoped.
