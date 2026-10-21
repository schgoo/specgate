# Changelog

Entries are one line per change. Rationale lives in the commit body, an ADR
under `docs/decisions/`, or `docs/roadmap.md`. Generate with `just changelog`.

## [Unreleased]

### Added

- One shared native golden bundle per component across all languages, including
  one canonical registry flat under `component/<id>/` and no per-language successful-component artifacts, selecting
  C# for paired fixtures and Rust for Rust-only fixtures, with independent strict
  transient capture and registry validation by both producers, canonical-byte
  anchoring, exact parity exceptions, Rust replay checks, and divergence witnesses (#47).
- C# capture compiles the sources once per batch: reflection reads the woven
  build's assembly and keeps every component's metadata (nested foreign ones
  included) through publication, instead of two further builds (#46).
- C# capture persistence failure is sticky, nested cross-component captures are
  discovered and linked-validated before publishing, multi-targeting is read from
  evaluated MSBuild properties, and `--filter` escapes `%` and `"` (#46).
- C# capture stops the batch when a test's TRX is missing or truncated (crashed
  host, bad filter), and parses the whole `--list-tests` output so a stray
  warning no longer truncates it; an empty listing is an error (#46).
- C# capture supports xUnit, MSTest and NUnit tests projects, detected from the
  evaluated package references (no new binding field); an unknown or mixed
  framework fails closed, and each framework is filtered on the property that
  holds its listed name (`DisplayName` for xUnit, `Name` for MSTest and NUnit)
  (#46).
- C# capture publishes a scenario only when the test's TRX outcome is `Passed`
  (skipped tests that completed an operation are not captured; strict capture
  fails), and capture and discovery now select the same sources `.csproj`
  regardless of directory listing order (#46).
- C# operations belong to exactly one component: `SpecOperationAttribute` no
  longer allows repeated use, and the weaver and `specgate-discovery` reject a
  method that declares several (#46).
- `specgate capture --impl csharp` builds the binding's `tests.projects` with the
  capture weaver injected, runs each listed test in its own `dotnet test`
  process, and emits validated bundles; the weaver, net8.0 runtime, and
  `SpecGateCapture.targets` ship beside the executable (`SPECGATE_WEAVER_PATH`
  overrides), and woven capture needs .NET 8+ (#46).
- `SpecGate.Runtime` targets net8.0 and net10.0 and starts capture from
  `SPECGATE_NATIVE_CAPTURE` through its own module initializer (#46).
- A plain CLR enum maps onto a CTSC tagged union of payload-free variants in
  declaration order, in both C# capture and C# discovery.
- Two declarations that resolve to one CTSC variant name are rejected, in C#
  capture and in discovery normalization for both languages, matching the rule
  already applied to record members.
- Negative discovery goldens pin all four `#[spec_setup]` collision rejections:
  unknown fills target, ambiguous return-type match, duplicate fills, and a
  second receiver producer.
- `#[spec_operation]` instruments a directly-awaited `async fn`, recording from
  first poll through completion.
- `finish_native_capture` closes outstanding operations with an
  `incomplete_capture` target fault and persists the sidecar.
- The capture sidecar is a continuously valid provisional snapshot, rewritten at
  each operation's recorded inputs and at every close.
- `specgate-discovery`, a focused crate owning one strict target-binding
  resolver shared by all CLI commands.

### Changed

- Retired duplicate C# woven behavioral tests and unused fixtures; setup rejection
  tests now exercise woven boundaries outside the captured native test project (#47).
- Bindings use `components.<id>.implementations.<label>` with `language`,
  `sources`, `tests`, and a language block; the CLI takes `--binding`
  (default `./specgate.yaml`), `--component`, and `--impl`, and `replay`
  takes the component from the bundle. The `package_root`, `targets`,
  `outputs`, file-wide `language`, `--target`, and positional binding are
  removed.
- The public Rust API renames `Target`, `ResolvedTarget`, and `TargetName` to
  `Implementation`, `ResolvedImplementation`, and `ImplementationLabel`, and
  `resolve_target` to `resolve_implementation`.
- `capture --out` is optional (default
  `./.specgate/captures/<component>/<impl>/<UTC run id>/`, gitignored) and
  `replay --out` defaults to `<capture-dir>/candidate-<impl>-<run id>.otlp.json`;
  both refuse existing output unless `--force`. The manifest `target.name` and
  trace `conformance.target.name` are the output directory name, and the
  replay candidate label is the output file stem. Rust goldens were
  regenerated with `target.name` `rust` (was `default`) and their digests
  changed; C# goldens are unchanged. `CapturePaths` carries only
  the binding; `out` and `force` are builder options, as for replay.
- The unused public `CaptureErrorKind::Selection` variant is removed.
- C# discovery runs the prebuilt `SpecGate.Discovery` tool instead of a C#
  program generated and compiled on every run, so the reflection code is built,
  analyzed, formatted, and unit tested by the ordinary C# gate.
- `[SpecEvent]` is valid on an enum declaration, which is what lets a plain CLR
  enum be discovered and captured as a named type.
- The packaged-context smoke compares NuGet package entry content rather than
  package bytes, which `dotnet pack` stamps with the wall clock.
- A failed replay reports its whole cause chain, so a runner failure names the
  candidate's own output instead of only the stage.
- Native capture rejects colliding `#[spec_setup]` declarations instead of
  dropping them, matching C# capture and discovery.
- Capture activation validates the whole linked registry, so a misconfigured
  component fails before any operation records a value.
- Native capture state is owned by a per-run `Send` collector rather than a
  thread-local, which is the precondition for carrying context into a future.
- Trace validation rejects a `conformance.operation` span carrying no completion
  or failure event unless its status is `OK`.
- Registry documents order `components` by ascending id in every encoder, so
  `discover` and `capture` emit byte-identical bytes.
- Component capture exports top-level operation subtrees verbatim instead of
  rejecting scenarios that call into other annotated components.
- The CTSC-native discovery, capture, and replay stack replaces the legacy
  verification stack.
- Rust and C# fixtures are reduced to stateless, rich-type, and setup-discovery
  coverage.

### Testing

- `fixture.async_fetch`, `fixture.extract`, `fixture.async_smol_timer`, and
  `fixture.async_tokio_timer` graduate from discovery-only rows to linked
  capture rows. No row carries `async-capture-unsupported`.
- `fixture.fallible_unit` drives its async operation but stays discovery-only on
  a new `observation-not-declared` limitation, pending M5.
- A regression test pins that two operations polled concurrently on one thread
  fail closed rather than recording a wrong trace.

### Removed

- Spec-driven validation, execution, matching, extraction, code generation,
  coverage, and self-hosting.
- Flat trace recording and table-driven mock instrumentation.
- The former harness/types crates and C# runtime/weaver projects.
- The standalone `specgate-annotations` facade; `specgate` is the sole facade.
