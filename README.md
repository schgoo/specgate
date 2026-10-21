# SpecGate

SpecGate captures native implementation behavior as deterministic CTSC
artifacts, then replays the same semantic inputs against another implementation.

```text
implementation metadata -> CTSC registry
ordinary tests -> reference capture
reference stimuli + candidate metadata -> candidate replay
```

## Commands

```powershell
specgate discover --binding test\behavior\specgate.yaml `
  --component fixture.stateless_add `
  --impl rust `
  --registry-id urn:ctsc:registry:fixture.stateless_add `
  --registry-version 0.1.0 `
  --out registry.ctsc.json

specgate capture --binding test\behavior\specgate.yaml `
  --component fixture.stateless_add `
  --impl rust `
  --out capture

specgate replay capture --binding test\behavior\specgate.yaml `
  --impl rust `
  --out candidate.otlp.json

specgate validate bundle capture

specgate compare capture\reference.otlp.json candidate.otlp.json `
  --registry capture\registry.ctsc.json
```

`capture --out` and `replay --out` are optional. Capture defaults to
`.specgate/captures/<component>/<impl>/<UTC run id>/`; replay defaults to
`<capture-dir>/candidate-<impl>-<run id>.otlp.json`. Both refuse existing
output unless `--force`, and the output name becomes the artifact's
`target.name`.

`discover` supports Rust link-time metadata and C# compiled-assembly reflection.
`capture` supports Rust and C#; `replay` currently supports Rust. `validate` natively checks CTSC
0.3 Registry, Trace Core, Linked, and capture-bundle semantics, including JSONL
traces and local or explicit registry imports. `compare` applies the fixed
`ctsc.strict/0.1.0` policy and reports stable semantic mismatch paths.
Capture bundles contain
`registry.ctsc.json`, `reference.otlp.json`, and `manifest.json`.
A directly-awaited async operation is captured natively, with recording
starting at the future's first poll. Async setups remain discoverable but
rejected by native capture, as do futures that migrate across executor threads.

Rust runner generation resolves the candidate's exact `specgate-runtime`
source through `cargo metadata`. `SPECGATE_RUNTIME_PATH` and
`SPECGATE_RUNTIME_VERSION` are identity-preserving assertions, not dependency
substitutions: path overrides must resolve to the candidate's exact Cargo
PackageId, and version overrides must match its crates.io package.

C# discovery runs the prebuilt `SpecGate.Discovery` tool. It is located beside
the running `specgate` executable; `SPECGATE_DISCOVERY_PATH` overrides that
with either the directory holding `SpecGate.Discovery.dll` or the assembly
path itself, and a relative override is resolved against the caller's working
directory. Working from a source checkout, `just discovery-tool` builds it and
every recipe that drives C# discovery points the override at that build.
Release artifacts ship that layout: each downloadable archive holds the
`specgate` executable and the tool's publish output beside it, so C# discovery
needs only a .NET runtime.

C# capture runs the binding's `tests.projects` through `dotnet` with the
capture weaver injected into the build, one `dotnet test` process per listed
test. The weaver, the net8.0 `SpecGate.Runtime`, and `SpecGateCapture.targets`
are located beside the `specgate` executable; `SPECGATE_WEAVER_PATH` overrides
that directory, and `just weaver-tool` builds it from a source checkout. Woven
capture needs a .NET 8+ SDK and runtime.

## Annotation surface

Rust implementations use `#[spec_operation]`, `#[spec_setup]`,
`#[derive(SpecEvent)]`, `#[spec_event]`, `spec_component!`, and `spec_trace!`.
Operations record typed inputs and terminal behavior at real invocation
boundaries. C# uses matching annotation metadata for discovery and IL weaving
for native capture.
Rust macros resolve the actual `specgate` dependency hygienically, including
renamed dependencies.

## Repository

- `rust/crates/specgate-runtime` — native capture and link-time metadata.
- `rust/crates/specgate-discovery` — bindings, Rust/C# discovery, normalization.
- `rust/crates/specgate-ctsc` — registry/trace encoding, native validation,
  strict comparison, and replay decoding.
- `rust/crates/specgate-cli` — `discover`, `capture`, `replay`, `validate`, and
  `compare`.
- [`test/behavior`](test/behavior/README.md) — behavior-first cases, native tests,
  shared expected bundles, matrix, and aggregate Rust/C# fixture projects.
- `test/Cargo.toml` — positive Rust fixture workspace, including the separate
  renamed-facade dependency witness.
- `test/rejection` — isolated fixtures discovery or the compiler must reject.
- `test/mechanics/csharp` — runtime, discovery, and weaver mechanics tests.
- `test/support/csharp` — shared C# test support.
- `csharp/SpecGate.Discovery` — the C# reflection tool the Rust driver invokes.
- `docs/ctsc` — committed CTSC contracts, validator, and corpus.

See the [documentation index](docs/README.md) for contracts, implementation
status, decision records, digests, and the agent delivery workflow.

## Golden matrix

`test/behavior/matrix.json` is hand-authored configuration: it accounts for
every annotated fixture source, names each component's language phases,
expected outcome, cross-language parity, reference-to-candidate replay checks,
and limitations, and
records where removed spec features now live.

Only declared `expected/` directories under `test/behavior/cases` and the Rust rejection project contain generated product output —
registries, reference traces, capture manifests, and normalized discovery
errors. **Never hand-edit a generated artifact.** Regenerate it:

```powershell
just ctsc-goldens-update   # rewrite the artifacts, then review the diff
just ctsc-goldens-check    # regenerate into scratch and byte-compare
```

`just ctsc-goldens-check` is part of `just check`. It regenerates every
artifact from a fresh toolchain run, validates each one with the native CTSC
registry, trace, linked, and bundle validators, byte-compares Rust and C#
registries wherever the matrix claims parity, replays every component the
matrix marks replayable, and rejects missing, stale, or undeclared goldens.
Matrix version `0.2.0` selects one shared native golden bundle per component
directly under `cases/<id>/expected/`, including the single canonical registry.
Successful components have no per-language golden directories. Discovery-only
components keep that registry without behavioral files. Paired fixtures explicitly select the current
C# endpoint; Rust-only fixtures select Rust. This is fixture configuration, not
a harness language policy. Both producers still run independent strict native
capture; nonselected bundles are validated in invocation scratch, not stored or
byte-compared as separate behavioral goldens. The selected full native registry
must byte-match the checked-in canonical; every other independent producer
registry must match that same oracle exactly or differ only by the exact known
parity exceptions. Exceptions and unsupported replay are explicit limitations,
not behavioral equivalence claims. The selected producer captures
directly into the case's `expected/` directory (native language retained,
`target.name` is its basename, `expected`).
Replay checks name the selected reference and candidate implementations, not
language pairs, and always resolve the shared bundle. Strict
comparison uses the reference scenario names carried through replay, not native
test-name matching. Unsupported replay does not remove native capture coverage.
Every captured bundle must link against its own registry; there is no linkage
exception. A parity exception is not a licence to differ: the row lists every
allowed difference as a JSON Pointer plus the exact value each language emits,
and the observed difference set must equal it. The harness also compares the
components each compiled target actually declares against the matrix rows, and
fails golden capture on any failing fixture test or discovered operation absent
from every trace. An intentional operation gap requires an exact
`captureExclusions` entry with a stable code and reason. Replay
`expectCategory` values and negative compiler-source attribution are equally
exact, so unrelated failures cannot satisfy a golden row.

Run the complete gate before committing:

```powershell
just check
```

Release changes should also run `just package-smoke`.

See [the migration status](docs/specgate-ctsc-migration.md) for current
limitations.
