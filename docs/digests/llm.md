# SpecGate LLM digest

> **Audience: SpecGate developers, not SpecGate users.** This digest is for
> agents changing the SpecGate repository itself. To use SpecGate on another
> codebase, follow [`docs/guides/README.md`](../guides/README.md) instead.

Operational summary for coding agents. This file is not independently
authoritative; update the source contract or implementation document first.

## Product

SpecGate discovers implementation metadata as a CTSC registry, captures
ordinary Rust or C# tests as a reference CTSC bundle, replays the reference stimuli
against a Rust candidate, validates the artifacts, and compares reference and
candidate traces with `ctsc.strict/0.1.0`.

The active CLI surface is `discover`, `capture`, `replay`, `validate`, and
`compare`. Rust and C# support discovery and capture; replay currently supports
Rust.

To use SpecGate on a codebase, follow `docs/guides/README.md` (agent quick
path, starting at step 0 for an existing API) and the facts in
`docs/reference/` (`cli.md`, `binding.md`, `value-mapping.md`, `bundle.md`,
`limitations.md`). CLI stdout lines are not yet a stable contract; exit codes
are 0 success, 1 failure or negative verdict, 2 usage error.

## Ownership boundaries

- `specgate-runtime` owns native capture, semantic values, and link-time
  operation and type metadata. Operation and setup finalization is explicit and
  consuming; destructors perform only infallible in-memory cleanup.
- `specgate-annotations-macros` instruments operation boundaries and emits
  setup and type metadata.
- `specgate` is the sole Rust annotation and runtime facade.
- `specgate-discovery` owns strict binding resolution (components and named implementations), Rust discovery builds,
  C# compiled-assembly reflection, raw invocation metadata, and deterministic
  normalization and setup folding. C# reflection runs in the prebuilt
  `csharp/SpecGate.Discovery` tool, invoked as
  `dotnet SpecGate.Discovery.dll <output-dir> <assembly> <assembly-out-dir> [component...]`.
  The tool emits raw per-component JSON only; it produces no CTSC artifacts.
  It is located beside the `specgate` executable unless
  `SPECGATE_DISCOVERY_PATH` names its directory or assembly path, which the
  justfile sets from `just discovery-tool`.
- C# capture (`specgate-cli`, `capture/csharp.rs`) builds each tests project once
  with `SpecGateCapture.targets` injected through
  `-p:CustomAfterMicrosoftCommonTargets`, so the sources project is IL-woven
  during that build and no project file is edited. Each listed test name
  (xUnit, MSTest or NUnit, detected from the tests project's package references)
  is one scenario, run by an exact `--filter` in its own `dotnet test` process
  with `SPECGATE_NATIVE_CAPTURE` set; a test-result log proves the filter
  selected exactly one test. The runtime-owned `[ModuleInitializer]` in
  `SpecGate.Runtime` (net8.0;net10.0) starts the session from that variable and
  rewrites the sidecar atomically once each operation's inputs are recorded and
  again at every operation close; a persistence failure poisons the session
  (later writes keep failing, and a marker on stderr makes the CLI reject the
  batch even if the test caught the exception). The weaver,
  runtime, and targets sit beside the executable, or in `SPECGATE_WEAVER_PATH`
  (the justfile sets it from `just weaver-tool`). Woven capture needs .NET 8+.
- `specgate-ctsc` owns registry and trace encoding, digest and linkage
  verification, replay models, validators, and strict comparison.
- `specgate-cli` exposes the product commands. Its Rust capture API takes an
  owned, validated `CaptureRequest` with `PathBuf` and semantic identity fields
  and returns a typed `CaptureError`; non-Unicode request paths are rejected and
  the CLI surface remains unchanged.
- Registry import URI handling depends on crates.io `templated_uri` 0.6 without
  default features. Its HTTP-oriented `Uri` parser is not used for CTSC file
  hints; a dedicated adapter validates authorities and preserves relative
  `file:` resolution, percent decoding, platform paths, and diagnostics.

## Invariants

- Behavior changes use focused CTSC registry, native trace, capture, replay,
  validator, or comparator tests. Do not add a separate behavioral assertion
  format or flat trace compatibility path.
- Capture observes real annotated operation boundaries through ordinary tests.
- The native capture sidecar is a provisional snapshot rewritten once each
  operation's declared inputs are recorded and again at every operation close,
  valid as of the last completed write and authoritative once the test process
  exits. Outstanding operations are projected into it with an
  `incomplete_capture` target fault from a clone, so the live logical clock
  never moves and completed recordings are unaffected.
- `specgate capture --component <id>` selects a scenario's *top-level*
  operations of that component and exports each of those subtrees verbatim.
  Nested calls into other annotated components keep their original parentage,
  nothing is ever reparented or promoted, and a scenario with no top-level
  operation of the component contributes nothing. The exported registry is the
  union of every component present in the filtered trace, so nested foreign
  operations stay declared and the bundle still links.
- Every captured bundle passes trace, linked, and bundle validation.
- A registry document orders its `components` array by ascending component id.
  No component is privileged by being the selected or root one. Single-schema
  and multi-schema encoding share this one rule, so `discover` and `capture`
  emit byte-identical bytes for the same component set.
- Setup selection is exact by component and operation; ambiguity is rejected.
  Setup construction inputs are folded into the operation input surface, and
  setup-filled parameters are omitted.
- Structured native values implement `ToNativeValue`, normally through
  `#[derive(SpecEvent)]`.
- C# attributes are metadata only; capture weaves its own build of the
  sources project. Preserve raw declaring type, method,
  signature, visibility, static/async/setup state, and exception metadata.
- Generated Rust runners resolve the candidate's exact `specgate-runtime`
  package through candidate-rooted `cargo metadata`. Runtime path and version
  overrides are identity-preserving assertions, not dependency substitutions.
- An `#[spec_operation] async fn` is captured. The macro rewrites it into a
  `fn` returning `impl Future`: it captures the caller's collector and parent
  operation at construction, opens its span on first poll, and reinstalls that
  context around every poll. Migrated futures therefore remain children of
  their construction parent, while interleaved siblings complete independently
  and retain correct parentage.
- Operation recording uses a synchronized, per-run `Send` collector. Setup
  input staging remains thread-local, so async `#[spec_setup]` is unsupported
  and rejected before capture; setup staging itself has no async support.
  Raw-thread-first/process-global activation is also unsupported.
- Abandonment while an async operation is still `Pending` is implemented: the
  span records `conformance.abandoned` with `UNSET` status, which is neither a
  completion nor a fault and does not propagate to containing spans. A drop
  during an unrelated unwind is still recorded as a fault, because
  `std::thread::panicking()` is checked before abandonment. *Top-level*
  abandonment — no enclosing and no subsequent annotated operation — is
  unsupported: no boundary follows to rewrite the provisional sidecar. More
  generally, `specgate capture` drops any decoded capture still carrying an
  `incomplete_capture` fault, since a failing test's sidecar is discarded before
  decoding and such a recording could never be finalized, instead of emitting a
  passing scenario as `error`. The dropped scenario is reported as a failed
  test, so sibling components in the same execution pass still bundle, and only
  a requested component left with no scenario fails the command; strict capture
  (golden generation) still fails the whole batch. The supported form of
  abandonment is nesting
  (`decisions/top-level-abandonment-unsupported.md`).
- C# async operations returning `Task`, `Task<T>`, `ValueTask`, or
  `ValueTask<T>` are woven and captured; cancellation is recorded as
  `conformance.abandoned`. Async setups, `async void`, `async` methods returning
  other awaitables, and async iterators are rejected by the weaver
  (`decisions/csharp-async-capture.md`).
- Rust `#[spec_operation]` and `#[spec_setup]` reject `const fn` at compile
  time; capture needs runtime state. Annotate a non-const wrapper and call it
  from tests (`guides/instrument-rust-crate.md`).
- Environment-driven capture atomically persists the cumulative provisional
  scenario after declared inputs and after every operation close. Outstanding
  operations are projected as target-observed `incomplete_capture` faults from
  a clone, without advancing the live logical clock. Persistence failure
  terminalizes the session and is promoted through a hidden marker to the
  independent CTSC `CaptureErrorKind::Execution`; Rust errors remain opaque.
  Generated operations explicitly finalize completion/fault boundaries and
  resume target panics; destructors never perform I/O. Manual sessions use
  `finish_native_capture`; no crash-durable or partial-output guarantee is made.
- Rust registry and capture APIs use semantic component/operation identities;
  capture requests own validated paths and reject non-Unicode request paths.
- Observation declarations and configurable comparison profiles are not
  implemented.

## Golden matrix

`test/behavior/matrix.json` is hand-authored and accounts for every
annotated fixture source, discovered component, negative fixture, parity
exception, replay limitation, capture exclusion, and removed-feature
replacement.

Only declared `expected/` directories under `test/behavior/cases` and the Rust rejection project contain generated product output. Use
`just ctsc-goldens-update`, review the artifact diff, then run
`just ctsc-goldens-check`. The check is part of `just check`.

The matrix is exact:

- declared and discovered components must match in both directions;
- every enumerated capture test must pass;
- every discovered operation must appear in a trace unless excluded by exact
  operation, stable code, and reason;
- parity differences must exactly equal the declared JSON Pointer and
  language-value set;
- replay checks identify reference and candidate implementations, not languages;
- matrix `0.2.0` selects exactly one golden bundle for all languages per component
  directly in `cases/<id>/expected/`, including the single canonical registry;
  no per-language successful-component artifacts or compatibility reader;
- discovery-only components select a successful discovery endpoint with only
  the shared registry, no behavioral files or checks;
- selected full native registry bytes must match the checked-in canonical in
  check mode, preventing simultaneous producer drift; independently generated
  other registries match that oracle exactly or by existing exact differences,
  preserving full dependency closure/order and Rust/C# side orientation;
- the three parity exceptions describe known non-equivalences; neither
  unsupported replay nor link-only checks establish behavioral equivalence;
- paired fixtures explicitly select C#, Rust-only fixtures select Rust; this is
  fixture configuration, not harness language policy;
- both producers still capture strictly; nonselected bundles live in invocation
  scratch and must pass native validation and exact operation/exclusion coverage;
- the selected producer writes directly to the case's `expected/` directory (no
  `reference` subdirectory), so native `target.name` is its basename, retaining
  its language and scenario names; never translate or relabel traces;
- checks must name the selected reference, always resolve the shared bundle,
  and replay on Rust candidates where supported, without
  matching independently captured test names;
- unsupported replay must fail with the declared stable category;
- build-negative compiler errors must have primary spans only in intentional
  sources.

## Current limitations

The detailed list is the single table in `docs/reference/limitations.md`;
architecture and status are in `docs/status.md`. Important current boundaries include:

- reference capture supports Rust libtest targets and C# native test projects;
- replay supports public Rust free functions and setup-backed instance methods,
  directly awaited async calls, explicit generic specializations, and structured
  results/errors with matching CTSC declarations;
- Rust test-only/private setup producers are resolved semantically in actual
  Cargo test targets; generated replay hooks run instead of ordinary tests.
  Each target declares `spec_component!` at its own crate root; the declaration
  emits the CLI-owned hook without SpecGate compiler wrappers or source copying/writes.
  Generated builds diagnose missing declarations even with explicit `spec` values;
  a hook-only default does not register an unused component.
  Configured compiler wrappers remain under normal Cargo handling.
  Construction inputs support primitives and recursive lists, sets, string-keyed
  maps, and optionals. Explicit property setups recursively fold nested opted-in
  records, including producer arguments, into flat semantic leaves. Duplicate
  leaf names and cyclic expansions are rejected; records inside containers are
  not decomposed. Equal receiver producer/input identities share state within a scenario;
- `discover`, `capture`, and `replay` reject overlapping commands against the
  same canonical package sources immediately and retain ownership through
  publication; reusable replay handles share the lease. Ordinary `cargo test`
  parallelism is unchanged;
- async/generic setups, cross-target setup environments, overlapping sibling
  scheduling, top-level abandonment, explicit type mappings, general record
  construction inputs, and C# candidate replay remain unsupported;
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
