# SpecGate CTSC Migration Status

## Legacy removal complete

The repository now uses CTSC-native discovery, capture, and replay. The former
spec parser, behavioral matcher, generated runners, case extraction, coverage,
self-hosting, flat event buffer, record-mode JSONL sink, mock runtime, C#
weaver/runtime, and associated product commands have been removed.

The active flow is:

```text
reference target
  -> discover semantic registry
  -> capture ordinary tests at native operation boundaries
  -> verify bundle digests and linkage
  -> statically link top-level stimuli to a candidate
  -> replay
  -> emit an independent candidate CTSC trace
  -> validate and compare with ctsc.strict/0.1.0
```

## Retained architecture

- `specgate-runtime`: native inputs, observations, result/empty/error/fault
  completion, deterministic sidecars, and link-time metadata.
- `specgate`: the sole published Rust annotation facade.
- `specgate-discovery`: one strict binding resolver, Rust link-time discovery,
  C# compiled-assembly reflection, raw native invocation metadata, normalized
  DTOs, component-scoped setup folding, and transitive dependency closure.
- `specgate-ctsc`: registry and native OTLP encoding, reusable CTSC Registry,
  Trace Core, Linked, and bundle validation, typed replay decoding,
  deterministic IDs, and strict differential comparison.
- `specgate-cli`: `discover`, `capture`, `replay`, `validate`, and `compare`.

Rust and C# registry output is byte-identical for the stateless, rich-type, and
setup-folding fixtures. Raw language-specific invocation metadata remains
separate from the language-neutral CTSC registry.

Discovery, capture, and replay scratch work uses invocation-unique
operating-system cache directories. Generated runners use the exact
`specgate-runtime` path or registry version resolved from the candidate's
`cargo metadata`; caller working directories are never searched. Advanced
tooling may explicitly override this with `SPECGATE_RUNTIME_PATH` or
`SPECGATE_RUNTIME_VERSION`, but only as an assertion selecting the same Cargo
PackageId. A path must resolve to that exact package identity, while a version
must match the candidate's crates.io package; mismatches fail before runner
generation to prevent split runtime/linkme/capture state.

## Captured input surface

Native capture records the same black-box input surface the registry declares.
A `#[spec_setup]` producer records its construction inputs while a capture
session is active or requested, the next invocation of that exact component and
operation adopts them, and the operation parameters those setups fill are left
out. Explicit `fills`, receiver producers, several producers per operation, and
stacked setup annotations all resolve exactly the way discovery folds them.
Attribution never crosses a component or operation boundary, and a duplicate
folded input name is reported rather than guessed. Ordinary runs are unchanged:
nothing is projected or recorded unless a capture session is active or
requested.

## Golden matrix

`test/goldens/ctsc/matrix.json` is the reviewable configuration that accounts
for the whole fixture corpus:

- **Component rows** name the component, its Rust and C# sources, the phases
  each language runs (`discover`, `capture`), the expected outcome, the
  cross-language parity mode, the bundle linkage mode (`linked` for every
  captured bundle, `not-applicable` for discovery-only rows), the replay mode,
  and any limitation that keeps a row from full coverage.
- **Negative rows** name the fixture that discovery or the compiler must
  reject, and the stable failure category its normalized `error.json` must
  carry. Every compiler error in a build negative must have primary spans
  exclusively in sources the row declares; unspanned errors and errors in
  other files are rejected even when the intended error is also present.
- **Capture coverage** requires every discovered semantic
  `(componentId, operationName)` to occur in at least one trace, including
  nested calls. A row may exclude only an exact operation through
  `captureExclusions`, with a stable code and reason; unknown or stale
  exclusions fail.
- **Replay limitations** use `replay.expectCategory`. An `unsupported` row
  passes only for that exact stable planner limitation, never for an unrelated
  bundle, discovery, linkage, or runner failure.
- **Parity exceptions** enumerate, in `parity.allowedDifferences`, every
  semantic difference the two registries are allowed to have: a JSON Pointer
  plus the exact value each language emits (including a `missing` sentinel and
  an `arrayLength` for differing array lengths). The observed recursive diff
  must equal that set exactly, so a new, changed, or disappearing difference —
  including an added or removed operation or type — fails the gate.
- **Replacement rows** record each removed spec feature — mocks, property
  cases, matcher operators, target routing, coverage, and run provenance — with
  the rationale and the exact CTSC-native fixture, comparator, or validator
  tests that replaced it.

Everything else under `test/goldens/ctsc` is generated product output and must
never be hand-edited. `just ctsc-goldens-update` rewrites it;
`just ctsc-goldens-check` (part of `just check`) regenerates into
repository-local scratch, validates every artifact with the native CTSC
validators, byte-compares parity registries, replays every replayable row and
strictly compares it against its reference, and rejects missing, stale, or
undeclared goldens.

Coverage is checked from the product's side, not only from the matrix's. Each
discovery pass reports the components the compiled target actually declares —
the Rust link-time registry for the Rust and negative bindings, and the C#
reflection run's own inventory of every `[SpecOperation]` component in the
built assembly — and the harness fails when that inventory and the matrix rows
differ in either direction. Golden capture is likewise strict: any enumerated
fixture test that fails fails the whole batch, even when a sibling test already
covers the same component. (`specgate capture` itself is unchanged and still
captures the tests that pass.)

Every captured Rust bundle must validate natively as a trace, link against the
registry discovered from the same sources, and pass bundle validation. There is
no linkage exception: a bundle that cannot link is a product defect, not a row
annotation.

One kind of exception is pinned rather than skipped, so closing the product gap
fails the gate until the matrix is updated: a `parity` exception asserts that
the Rust and C# registries still differ in exactly the declared ways.

## Current limitations

- Reference capture supports Rust libtest targets only.
- Candidate replay supports synchronous public Rust free functions with
  lossless primitive inputs.
- Setup-backed methods, async calls, structured replay values, explicit
  mappings, and C# replay are not implemented.
- Async metadata is retained for linking, but native capture rejects async
  operations before polling until capture context can propagate task-safely.
  An async setup is not instrumented at all, so capture rejects the whole
  component up front rather than encoding a bundle without its inputs.
- Native validation supports JSON and JSONL traces, registry imports, exact
  capture-bundle integrity, and linked type checking. Bundle validation is
  intentionally independent from replay's narrower invocation decoder.
- Differential comparison currently implements the fixed
  `ctsc.strict/0.1.0` policy. Multiple-run selection, overlapping sequential
  children, and duplicate parallel branch identities are rejected as
  unsupported or ambiguous, as permitted by the policy.
- Replay validates nested operations as observed behavior but invokes only
  top-level reference operations.
- `spec_trace!` observations are captured but never declared, because discovery
  has no link-time observation metadata. A component that emits an observation
  cannot produce a linkable reference bundle, so CTSC-native fixtures express
  intermediate behavior as nested public operations instead.
- Components that declare any async operation or async setup are
  discovery-only: capture rejects an async operation before polling and
  rejects an async setup before it builds anything, so no reference bundle
  exists. The golden matrix asserts this for every row it captures.
- Discovery rejects duplicate operation identity, orphan setups, method
  operations without a receiver setup, operations on private functions, and the
  dynamic runtime `Value`, because none of them can describe a well-formed CTSC
  component surface.
