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
specgate discover test\bindings\rust.yaml `
  --component fixture.stateless_add `
  --registry-id urn:ctsc:registry:fixture.stateless_add `
  --registry-version 0.1.0 `
  --out registry.ctsc.json

specgate capture test\bindings\rust.yaml `
  --component fixture.stateless_add `
  --out capture

specgate replay capture test\bindings\rust.yaml `
  --out candidate.otlp.json

specgate validate bundle capture

specgate compare capture\reference.otlp.json candidate.otlp.json `
  --registry capture\registry.ctsc.json
```

`discover` supports Rust link-time metadata and C# compiled-assembly reflection.
`capture` and `replay` currently support Rust. `validate` natively checks CTSC
0.2 Registry, Trace Core, Linked, and capture-bundle semantics, including JSONL
traces and local or explicit registry imports. `compare` applies the fixed
`ctsc.strict/0.1.0` policy and reports stable semantic mismatch paths.
Capture bundles contain
`registry.ctsc.json`, `reference.otlp.json`, and `manifest.json`.
Async operations and async setups are discoverable but rejected by native
capture until capture context can propagate safely across executor threads.

Rust runner generation resolves the candidate's exact `specgate-runtime`
source through `cargo metadata`. `SPECGATE_RUNTIME_PATH` and
`SPECGATE_RUNTIME_VERSION` are identity-preserving assertions, not dependency
substitutions: path overrides must resolve to the candidate's exact Cargo
PackageId, and version overrides must match its crates.io package.

## Annotation surface

Rust implementations use `#[spec_operation]`, `#[spec_setup]`,
`#[derive(SpecEvent)]`, `#[spec_event]`, `spec_component!`, and `spec_trace!`.
Operations record typed inputs and terminal behavior at real invocation
boundaries. C# retains matching annotation metadata for registry discovery.
Rust macros resolve the actual `specgate` dependency hygienically, including
renamed dependencies.

## Repository

- `rust/crates/specgate-runtime` — native capture and link-time metadata.
- `rust/crates/specgate-discovery` — bindings, Rust/C# discovery, normalization.
- `rust/crates/specgate-ctsc` — registry/trace encoding, native validation,
  strict comparison, and replay decoding.
- `rust/crates/specgate-cli` — `discover`, `capture`, `replay`, `validate`, and
  `compare`.
- `test/rust/crates/specgate-ctsc-fixtures` — focused Rust fixtures.
- `test/rust/negative-fixtures` — fixtures discovery or the compiler must reject.
- `test/csharp/SpecGate.CtscFixtures` — annotation-only C# fixtures.
- `test/goldens/ctsc` — the golden matrix and its generated artifacts.
- `docs/ctsc` — committed CTSC contracts, validator, and corpus.

## Golden matrix

`test/goldens/ctsc/matrix.json` is hand-authored configuration: it accounts for
every annotated fixture source, names each component's language phases,
expected outcome, cross-language parity, replay support, and limitations, and
records where removed spec features now live.

Everything else under `test/goldens/ctsc` is generated product output —
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
