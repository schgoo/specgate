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
  completion, explicit fallible operation/setup finalization, deterministic
  cumulative sidecars, and link-time metadata.
- `specgate`: the sole published Rust annotation facade.
- `specgate-discovery`: one strict binding resolver, Rust link-time discovery,
  C# compiled-assembly reflection, raw native invocation metadata, normalized
  DTOs, component-scoped setup folding, and transitive dependency closure.
- `specgate-ctsc`: registry and native OTLP encoding, reusable CTSC Registry,
  Trace Core, Linked, and bundle validation, typed replay decoding,
  deterministic IDs, and strict differential comparison.
- `specgate-cli`: `discover`, `capture`, `replay`, `validate`, and `compare`.

Registry import hints use the approved `templated_uri` dependency for authority
validation while retaining a file-specific adapter for CTSC's relative `file:`
form. Resolution remains local-only and preserves the existing percent-decoding,
platform path, and diagnostic behavior. See
[the ratified decision](decisions/templated-uri-for-registry-imports.md).

Rust exposes `ComponentId`, `OperationName`, and `TargetName` as unrestricted
semantic string identities. The public capture library API accepts one owned
`CaptureRequest`, whose constructor validates that its `PathBuf` filesystem
inputs are UTF-8 representable, and returns `Result<CaptureReport, CaptureError>`. Capture failures retain actionable text
through an opaque `ohno` error. Machine-readable capture, discover, and replay
failure stages are separate protocol data types whose established CTSC wire
names remain unchanged. The CLI syntax, formatting, and exit codes remain
unchanged. See [the capture API decision](decisions/rust-semantic-identities-and-capture-api.md)
and [the opaque-error decision](decisions/opaque-rust-errors.md).

Rust and C# registry output is byte-identical for the stateless, rich-type, and
setup-folding fixtures. Raw language-specific invocation metadata remains
separate from the language-neutral CTSC registry.

Environment-driven Rust capture closes each top-level operation in memory and
then atomically persists the complete capture-so-far. Persistence failures
terminalize the session and carry a hidden runtime/facade marker that the parent
CLI reports as an execution failure rather than skipping as an ordinary failed
test. Generated code explicitly completes a target unwind as the existing fault
and then resumes the original panic payload. Operation and deferred-setup
destructors perform no I/O, panic, or stderr output; manual sessions still end
through `finish_native_capture`. There is no crash-durable or partial-output
guarantee. See
[the ratified decision](decisions/native-capture-finalization.md).

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

## Provisional capture sidecars

`specgate capture` runs ordinary tests and never signals "the test ended", so
the runtime cannot know when a recording is final. The per-test sidecar is
therefore a *provisional snapshot*: it is rewritten once each operation's
declared inputs are recorded and again at every operation close, and it is only
valid as of the last completed write. The last write before the test process
exits is the authoritative recording.

The snapshot is taken after input recording rather than at operation start
because the registry declares each operation's inputs: a span captured before
`record_input` has run carries an empty input surface and fails linked
validation against its own registry.

While operations are still outstanding, the snapshot is projected from a clone
of the session on which every outstanding operation is closed with an
`incomplete_capture` target fault, so the file always encodes well-formed spans
and a leaked operation scope leaves a linkable bundle instead of vanishing. The
clone's logical clock and event order advances are discarded, so a completed
recording is byte-identical to one that never snapshotted. Nothing reads the
sidecar while the process runs, so transient mid-operation inaccuracy is
expected.

## Component capture scope

`specgate capture --component <id>` anchors the export on the selected
component's *top-level* operations — those whose parent is the scenario span.
Each such span contributes its transitive subtree verbatim: nested calls into
other annotated components stay in the trace with their original parentage, and
no span is ever reparented or promoted. A top-level operation of another
component is dropped together with its whole subtree, and a scenario with no
top-level operation of the selected component contributes nothing.

Because a filtered trace can therefore contain operations of several
components, the bundle's registry is the union of every component present in
the filtered trace, encoded under the selected component's
`urn:ctsc:registry:<component>` id. Nested foreign operations stay declared, so
the bundle continues to satisfy trace, linked, and bundle validation.

A single-component capture emits the same registry bytes as `discover` for that
component, because both encoders share the one component-ordering rule in
[`ctsc/registry.md`](ctsc/registry.md) §3.1. That ordering rule is itself a
change: a component whose dependency ids sort before its own now emits a
different component order, and therefore a different registry digest, than
earlier releases that placed the selected component first. The fixture corpus
contains no such component, so no golden artifact changed.

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
- An async operation is captured: the macro rewrites the annotated `async fn`
  into a `fn` returning `impl Future`, which captures the caller's operation at
  construction, re-installs it around every poll, begins recording at first
  poll, and holds the scope across the body's awaits. A migrated future records
  under the operation that built it and starts no second session on the
  resuming thread, and interleaved operations both record complete spans. An
  async operation dropped while still `Pending` records
  `conformance.abandoned` with `UNSET` status, which does not propagate to the
  containing spans; a future abandoned on a thread that is already unwinding
  for an unrelated reason is recorded as `specgate.unexpected_target_fault`
  instead, because `std::thread::panicking()` is checked first so that a
  genuinely panicking async body is never downgraded to merely abandoned. Not
  covered: operations first
  reached on a raw `std::thread::spawn`ed thread. An async setup is not
  instrumented at all, so capture rejects
  the whole component up front rather than encoding a bundle without its
  inputs. The ratified design for the remaining work is
  [`decisions/async-capture-context.md`](decisions/async-capture-context.md).
- Native validation supports JSON and JSONL traces, registry imports, exact
  capture-bundle integrity, and linked type checking. Bundle validation is
  intentionally independent from replay's narrower invocation decoder.
- Differential comparison currently implements the fixed
  `ctsc.strict/0.1.0` policy. Multiple-run selection, overlapping sequential
  children, and duplicate parallel branch identities are rejected as
  unsupported or ambiguous, as permitted by the policy. `ctsc.strict/0.1.0` has
  no abandonment-specific handling beyond generic event matching: an abandoned
  operation differs from a completed one only because event names must match
  and a missing or additional event is a mismatch.
- Replay validates nested operations as observed behavior but invokes only
  top-level reference operations.
- `spec_trace!` observations are captured but never declared, because discovery
  has no link-time observation metadata. A component that emits an observation
  cannot produce a linkable reference bundle, so CTSC-native fixtures express
  intermediate behavior as nested public operations instead.
- Components that declare an async setup are discovery-only: capture rejects
  them before it builds anything, so no reference bundle exists. The golden
  matrix asserts this for every row it captures. An async operation no longer
  forces discovery-only status, but its bundle cannot be replayed, because the
  replay planner emits only synchronous candidate calls. A trace that
  interleaves two sibling operations is additionally unreplayable, because
  strict comparison rejects overlapping sequential children. See
  [`decisions/async-capture-context.md`](decisions/async-capture-context.md)
  for the ratified capture-context design.
- Discovery rejects duplicate operation identity, orphan setups, method
  operations without a receiver setup, operations on private functions, and the
  dynamic runtime `Value`, because none of them can describe a well-formed CTSC
  component surface.
