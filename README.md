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
```

`discover` supports Rust link-time metadata and C# compiled-assembly reflection.
`capture` and `replay` currently support Rust. Capture bundles contain
`registry.ctsc.json`, `reference.otlp.json`, and `manifest.json`.
Async operations are discoverable but rejected by native capture until capture
context can propagate safely across executor threads.

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
- `rust/crates/specgate-ctsc` — registry/trace encoding and replay decoding.
- `rust/crates/specgate-cli` — `discover`, `capture`, and `replay`.
- `test/rust/crates/specgate-ctsc-fixtures` — focused Rust fixtures.
- `test/csharp/SpecGate.CtscFixtures` — annotation-only C# fixtures.
- `docs/ctsc` — committed CTSC contracts, validator, and corpus.

Run the complete gate before committing:

```powershell
just check
```

Release changes should also run `just package-smoke`.

See [the migration status](docs/specgate-ctsc-migration.md) for current
limitations.
