# SpecGate architecture and current status

This page describes what is implemented today and how the pieces fit together.
For user-facing limits, see [`reference/limitations.md`](reference/limitations.md).
For task-oriented instructions, see [`guides/`](guides/README.md).

## Flow

SpecGate discovers, captures, and replays natively against the CTSC contracts:

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

The former spec parser, behavioral matcher, generated runners, case
extraction, coverage, self-hosting, flat event buffer, record-mode JSONL sink,
mock runtime, and associated product commands were removed. The C# weaver and
runtime were removed with them and later rebuilt for CTSC-native capture.

## Components

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
- `specgate-cli`: `discover`, `capture`, `replay`, `validate`, and `compare`
  (see [`reference/cli.md`](reference/cli.md)).
  `capture` runs Rust through `cargo` and C# through `dotnet` (see
  [`decisions/csharp-capture-cli.md`](decisions/csharp-capture-cli.md)). C#
  capture builds each `tests.projects` entry once with the capture weaver
  injected into the build, lists its tests, and runs each listed test as one
  scenario (xUnit, MSTest and NUnit are supported and detected from package
  references) in its own `dotnet test` process. It requires the .NET 8+ runtime
  and SDK; the weaver, the net8.0 runtime, and `SpecGateCapture.targets` ship
  beside the `specgate` executable, and `SPECGATE_WEAVER_PATH` overrides that
  directory. `replay` supports Rust candidates only, and `tests.command`
  capture is not yet supported.
- `csharp/SpecGate.Annotations` (NuGet), `csharp/SpecGate.Discovery`,
  `csharp/SpecGate.Weaver`, and `csharp/SpecGate.Runtime`: C# annotation
  metadata, the prebuilt reflection tool, the build-time IL weaver, and the
  capture runtime. Only `SpecGate.Annotations` is a NuGet package; the other
  three ship beside the `specgate` executable (see
  [`guides/install.md`](guides/install.md)).

Registry import hints use the approved `templated_uri` dependency for authority
validation while retaining a file-specific adapter for CTSC's relative `file:`
form. Resolution remains local-only and preserves the existing percent-decoding,
platform path, and diagnostic behavior. See
[the ratified decision](decisions/templated-uri-for-registry-imports.md).

Rust exposes `ComponentId`, `OperationName`, and `ImplementationLabel` as
unrestricted semantic string identities. The public capture library API
accepts one owned `CaptureRequest`, whose constructor validates that its
`PathBuf` filesystem inputs are UTF-8 representable, and returns
`Result<CaptureReport, CaptureError>`. Capture failures retain actionable text
through an opaque `ohno` error. Machine-readable capture, discover, and replay
failure stages are separate protocol data types whose established CTSC wire
names remain unchanged. See
[the capture API decision](decisions/rust-semantic-identities-and-capture-api.md)
and [the opaque-error decision](decisions/opaque-rust-errors.md).

Rust and C# registry output is byte-identical for the stateless, rich-type, and
setup-folding fixtures. Raw language-specific invocation metadata remains
separate from the language-neutral CTSC registry.

Environment-driven Rust capture atomically persists the capture-so-far as a
provisional snapshot (see [below](#provisional-capture-sidecars)). Persistence
failures terminalize the session and carry a hidden runtime/facade marker that
the parent CLI reports as an execution failure rather than skipping as an
ordinary failed test. Generated code explicitly completes a target unwind as
the existing fault and then resumes the original panic payload. Operation and
deferred-setup destructors perform no I/O, panic, or stderr output; manual
sessions still end through `finish_native_capture`. There is no crash-durable
or partial-output guarantee. See
[the ratified decision](decisions/native-capture-finalization.md).

Discovery, capture, and replay scratch work uses invocation-unique
operating-system cache directories (`SPECGATE_CACHE_DIR` overrides the root).
Generated runners use the exact `specgate-runtime` path or registry version
resolved from the candidate's `cargo metadata`; caller working directories are
never searched. Advanced tooling may explicitly override this with
`SPECGATE_RUNTIME_PATH` or `SPECGATE_RUNTIME_VERSION`, but only as an assertion
selecting the same Cargo PackageId. A path must resolve to that exact package
identity, while a version must match the candidate's crates.io package;
mismatches fail before runner generation to prevent split
runtime/linkme/capture state.

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

## Candidate replay and explicit generic operations

Rust candidate replay supports public free functions and setup-backed instance
methods, including directly awaited async calls and explicit generic
specializations. Operation and setup construction inputs remain lossless
primitives. Structured results and declared errors, including collections of
annotated records, use the existing CTSC projection and linked validators.
Reachable named declarations, including nested observed outcomes, must match
the reference. Replay invokes only top-level reference operations; nested
operations remain observed behavior.

Discovery enumerates actual Cargo libtest targets, including private
`#[cfg(test)]` and integration-test producers. Replay resolves annotated setups
by semantic component, operation, and receiver or parameter role. It invokes a
generated entry in the selected Cargo target, not ordinary candidate tests.
Producers must be synchronous nongeneric free functions with supported primitive
construction inputs and owned, downcastable outputs. A replay cannot combine
setup environments from different Cargo test targets.

CLI-owned Rust test builds add their crate-root hook only in invocation scratch.
A dependency-only compiler pass identifies the source files to mirror with
their relative layout intact. Read-only sources are supported; interrupted
builds leave no temporary roots beside user code. Candidate-configured workspace
wrappers are chained rather than replaced, and Cargo retains ordinary outer
compiler wrappers. Cargo still needs writable build/cache directories and may
need a writable lockfile when its dependency resolution changes.

Equal receiver producer identities and construction inputs share a receiver
within one scenario. Separate scenario scopes release receivers and setup
guards before the next scenario. Concrete downcasts and calls stay in lexical
bridges: method specializations resolve in the operation module, not the
receiver producer module. Plain test-only operations also select their recorded
Cargo target, and integration-local module paths are checked against that
target's compiler-reported crate root.

Each generic specialization declares its own semantic operation name and
concrete type arguments. Rust accepts repeatable annotations such as
`#[spec_operation("find_items", types = "crate::Item")]`; multiple type parameters
use a comma-separated `types` string. C# uses
`[SpecOperation("find_items", Spec = "example.client", TypeArguments = new[] { typeof(Item) })]`.
The original generic constraints remain, including Rust's `'static` requirement
and C# constraints such as `where T : notnull`. Discovery publishes concrete
signatures without inferring specializations from tests. Calling an undeclared
specialization while recording fails explicitly. Open generic declaring types,
generic setups, and Rust const-generic operation specializations are unsupported.
Rust substitutions that change syntactic completion channels are rejected,
including `Option<T>` specialized to `Option<()>` and `T` specialized to
`Result<i32, String>`; use a concrete return signature. This check does not
resolve aliases or associated-type projections.

The paired CLI fixture captures a C# generic async client method and replays
it against a Rust method returning `Vec<Item>`. Private Rust setup producers in
an arbitrary integration-test target reconstruct the loopback HTTP environment.
Repeated calls retain receiver state, and strict comparison checks the typed
collection contents. See the
[accepted replay decision](decisions/typed-async-client-replay.md) and
[limitations](reference/limitations.md).

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

An `incomplete_capture` fault that *survives* into a decoded sidecar is a
different matter. Because a failing test's sidecar is deleted before it is ever
decoded, such a fault means a test that reported success left a recording that
could never be finalized. More than one shape produces it — a top-level
abandonment, which no later operation boundary repairs, or a target that exits
the process inside an open operation — so detection keys on the fact rather
than the cause. `specgate capture` rejects that capture with an actionable
diagnostic rather than emitting a bundle that reports `error` for a scenario
whose test passed; see
[`decisions/top-level-abandonment-unsupported.md`](decisions/top-level-abandonment-unsupported.md).

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

`test/behavior/matrix.json` is the reviewable configuration that accounts
for the whole fixture corpus (see [`../test/behavior/README.md`](../test/behavior/README.md)):

- **Component rows** name the component, its Rust and C# sources, the phases
  each language runs (`discover`, `capture`), the expected outcome, the
  cross-language parity mode, the bundle linkage mode (`linked` for every
  captured bundle, `not-applicable` for discovery-only rows),
  reference-to-candidate replay checks, and any limitation that keeps a row
  from full coverage.
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
- **Replay limitations** attach an exact `expectCategory` to each
  reference-to-candidate check. An `unsupported` check passes only for that
  exact stable planner limitation, never for an unrelated bundle, discovery,
  linkage, or runner failure.
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

Only declared `expected/` directories under `test/behavior/cases` and the Rust
rejection project contain generated product output and must never be
hand-edited. `just ctsc-goldens-update` rewrites it; `just ctsc-goldens-check`
(part of `just check`) regenerates into repository-local scratch, validates
every artifact with the native CTSC validators, byte-compares parity
registries, replays every replayable row and strictly compares it against its
reference, and rejects missing, stale, or undeclared goldens.

The matrix declares one shared binding, `test/behavior/specgate.yaml`
(shape per [the binding ADR](decisions/binding-schema-v1.md)), in place of
the earlier per-language `bindings` map of binding files. Each language row
names a binding implementation identity; its language is a property, not a
constraint on that label. The component is the row's component. The harness
resolves rows through the binding and groups them by implementation `sources`,
so rows that share sources still share one discovery and capture batch.
`checks` lists directed reference/candidate implementation labels with their
replay policies. Parity `allowedDifferences`, exact unsupported categories,
`captureExclusions`, and negative-row guarantees are preserved. The capture
manifest `target.name` and `conformance.target.name` trace attribute come from
the output directory name, not directly from the implementation identity.
The internal matrix version is `0.2.0` (no compatibility reader). Every captured
component selects exactly one successful discover+capture endpoint through
`reference: { implementation, artifacts }`. Its inventory is exactly
`manifest.json`, `reference.otlp.json`, and `registry.ctsc.json`, published
under `cases/<id>/expected/` directly, with no `reference` subdirectory. There
are no per-language successful-component artifacts, including registries. A
discovery-only component selects one successful discovery endpoint and retains
only the shared registry, no behavioral files or checks. Negative errors are
unchanged.

The fixture configuration selects C# for paired captured components and Rust
for Rust-only ones; that is not harness language policy. The selected native
capture writes directly into the case's `expected/` directory, naturally
encoding `target.name` as its basename (`expected`) while retaining its native
language and scenario names. No trace is copied, translated, or relabelled.
Both producers keep their capture phases. Nonselected producers run
independent strict native capture under invocation scratch
(`producer-captures/<row>/<implementation-directory>`), with the same registry,
Trace Core, Linked, bundle-digest, and exact operation/exclusion validation.
Their behavioral bundles are not separately byte-checked or stored. Every
producer's full native registry is obtained independently in scratch,
preserving dependency closure and ordering. Never substitute the canonical for
a producer's own bundle linkage validation. In check mode, the selected native
registry must byte-match the single checked-in canonical registry; compare
every other producer to that same anchored oracle, not just to fresh producer
output. Byte-identical rows require exact bytes. Exception rows require exactly
the existing `parity.allowedDifferences`, with Rust/C# sides preserved
regardless of which endpoint was selected. The three exception rows describe
precise known non-equivalences, not blanket equivalence passes.

Captured rows require checks whose `reference` equals the selected endpoint;
each check resolves the shared reference directory. Nonselected producers need
no replay check of their own. Discovery-only components have a registry-only
reference and no checks; negative and replacement rows have no reference or
checks. Candidate traces also remain in scratch. Unsupported and link-only
checks do not establish behavioral equivalence.

Coverage is checked from the product's side, not only from the matrix's. Each
discovery pass reports the components the compiled target actually declares —
the Rust link-time registry for the Rust and negative implementations, and the
C# reflection run's own inventory of every `[SpecOperation]` component in the
built assembly — and the harness fails when that inventory and the matrix rows
differ in either direction. Golden capture is likewise strict: any enumerated
fixture test that fails fails the whole batch, even when a sibling test already
covers the same component. (`specgate capture` itself is unchanged and still
captures the tests that pass.)

Every captured Rust or C# bundle must validate natively as a trace, link
against its own discovered registry, and pass bundle validation. C# reflection
reads the same woven assembly that its ordinary tests execute, not a separate
build. There is no linkage exception: a bundle that cannot link is a product
defect, not a row annotation. C# traces and manifests come from the retained C#
native tests, whether selected for storage or validated only transiently.
Replay checks identify reference and candidate implementation versions through
binding labels, not language pairs. Supported C# recordings replay on Rust
candidates and compare strictly while preserving reference scenario names;
unsupported replay does not suppress native capture. See
[the golden parity decision](decisions/csharp-golden-replay-parity.md).

One kind of exception is pinned rather than skipped, so closing the product gap
fails the gate until the matrix is updated: a `parity` exception asserts that
the Rust and C# registries still differ in exactly the declared ways.
