# SpecGate LLM digest

Operational summary for coding agents. This file is not independently
authoritative; update the source contract or implementation document first.

## Product

SpecGate discovers implementation metadata as a CTSC registry, captures
ordinary Rust tests as a reference CTSC bundle, replays the reference stimuli
against a Rust candidate, validates the artifacts, and compares reference and
candidate traces with `ctsc.strict/0.1.0`.

The active CLI surface is `discover`, `capture`, `replay`, `validate`, and
`compare`. Rust and C# support discovery; capture and replay currently support
Rust.

## Ownership boundaries

- `specgate-runtime` owns native capture, semantic values, and link-time
  operation and type metadata.
- `specgate-annotations-macros` instruments operation boundaries and emits
  setup and type metadata.
- `specgate` is the sole Rust annotation and runtime facade.
- `specgate-discovery` owns strict binding resolution, Rust discovery builds,
  C# compiled-assembly reflection, raw invocation metadata, and deterministic
  normalization and setup folding.
- `specgate-ctsc` owns registry and trace encoding, digest and linkage
  verification, replay models, validators, and strict comparison.
- `specgate-cli` exposes the product commands.

## Invariants

- Behavior changes use focused CTSC registry, native trace, capture, replay,
  validator, or comparator tests. Do not add a separate behavioral assertion
  format or flat trace compatibility path.
- Capture observes real annotated operation boundaries through ordinary tests.
- Every captured bundle passes trace, linked, and bundle validation.
- Setup selection is exact by component and operation; ambiguity is rejected.
  Setup construction inputs are folded into the operation input surface, and
  setup-filled parameters are omitted.
- Structured native values implement `ToNativeValue`, normally through
  `#[derive(SpecEvent)]`.
- C# annotations are metadata only. Preserve raw declaring type, method,
  signature, visibility, static/async/setup state, and exception metadata.
- Generated Rust runners resolve the candidate's exact `specgate-runtime`
  package through candidate-rooted `cargo metadata`. Runtime path and version
  overrides are identity-preserving assertions, not dependency substitutions.
- Async operations remain discoverable, but native capture rejects them before
  polling while capture state is thread-local.
- Observation declarations and configurable comparison profiles are not
  implemented.

## Golden matrix

`test/goldens/ctsc/matrix.json` is hand-authored and accounts for every
annotated fixture source, discovered component, negative fixture, parity
exception, replay limitation, capture exclusion, and removed-feature
replacement.

Everything else under `test/goldens/ctsc` is generated product output. Use
`just ctsc-goldens-update`, review the artifact diff, then run
`just ctsc-goldens-check`. The check is part of `just check`.

The matrix is exact:

- declared and discovered components must match in both directions;
- every enumerated capture test must pass;
- every discovered operation must appear in a trace unless excluded by exact
  operation, stable code, and reason;
- parity differences must exactly equal the declared JSON Pointer and
  language-value set;
- unsupported replay must fail with the declared stable category;
- build-negative compiler errors must have primary spans only in intentional
  sources.

## Current limitations

The detailed and authoritative list is in
`docs/specgate-ctsc-migration.md`. Important current boundaries include:

- reference capture is limited to Rust libtest targets;
- replay supports synchronous public Rust free functions with lossless
  primitive inputs;
- setup-backed methods, async calls, structured replay values, explicit
  mappings, and C# replay are not implemented;
- `spec_trace!` observations are captured but have no registry declarations,
  so observation-emitting components cannot produce linkable bundles;
- replay invokes top-level reference operations and validates nested operations
  only as observed behavior.

## Decision boundaries

Require an explicit human owner before changing normative CTSC shapes or
validation, public CLI shape, capture/replay identity and stimulus semantics,
comparison policy semantics, runtime package identity, or golden-matrix
declaration meaning.

## Validation

Use focused `just` recipes while iterating. Run `just check` from the repository
root before commit or handoff. Release and packaging changes also run
`just package-smoke`.

Crate READMEs are generated from crate-level `lib.rs` documentation. Edit the
crate docs and run `just readme`; do not edit generated crate READMEs directly.
Use `ohno` for new Rust error types. Use LF except for `.ps1`, `.cmd`, and
`.bat`.
