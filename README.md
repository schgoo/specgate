# SpecGate

SpecGate records how an implementation behaves in its own tests and checks that
another implementation behaves the same way.

```text
annotated code           -> CTSC registry   (discover)
ordinary tests           -> reference trace (capture)
recorded inputs + port   -> candidate trace (replay)
reference vs candidate   -> verdict         (compare)
```

You annotate operations in Rust or C#, run `specgate capture` over your existing
tests, and get a validated bundle in the
[CTSC](docs/ctsc/registry.md) interchange format. `specgate replay` runs the
recorded inputs against a Rust candidate, and `specgate compare` reports any
behavioral difference.

## Install

See [`docs/guides/install.md`](docs/guides/install.md) for the CLI, the
`specgate` crate, and the `SpecGate.Annotations` NuGet package.

## Quickstart

**Rust:** annotate with `spec_component!` and `#[spec_operation]`
([guide](docs/guides/instrument-rust-crate.md)).
**C#:** annotate with `[SpecOperation(..., Spec = "...")]`
([guide](docs/guides/instrument-csharp-assembly.md)).
Declare both in `specgate.yaml` ([guide](docs/guides/write-a-binding.md)), then:

```sh
specgate capture --component <id> --impl <reference-label> --out capture
specgate validate bundle capture
specgate replay capture --impl <rust-label> --out candidate.otlp.json
specgate compare capture/reference.otlp.json candidate.otlp.json --registry capture/registry.ctsc.json
```

- `--impl` is required whenever the binding declares more than one
  implementation for the component.
- `capture --out` defaults to `.specgate/captures/<component>/<impl>/<UTC run id>/`.
- `replay --out` defaults to `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`.
- Exit code 0 means success, valid, or equivalent
  ([CLI reference](docs/reference/cli.md)).

## What works today

| | Rust | C# |
| --- | --- | --- |
| Discover | Yes | Yes |
| Capture | Yes, including `async fn` | Yes, including `Task` and `ValueTask` |
| Async abandonment | Recorded when nested in a completing operation | Cancellation recorded as abandonment |
| Replay as candidate | Free synchronous functions | No |
| Validate, compare | Language-neutral | Language-neutral |

Details: [`docs/status.md`](docs/status.md) and
[`docs/reference/limitations.md`](docs/reference/limitations.md).

## Documentation

- [Guides](docs/guides/README.md): task-oriented, for people and agents.
- [Reference](docs/README.md#reference): CLI, binding, value mapping, bundle,
  limitations.
- [CTSC contracts](docs/ctsc/): the normative interchange format.
- [Documentation map](docs/README.md): everything else, including decisions and
  contributor docs.

## Repository

- `rust/crates/specgate` — the annotation facade crate users depend on.
- `rust/crates/specgate-runtime` — native capture and link-time metadata.
- `rust/crates/specgate-annotations-macros`, `specgate-annotations-macros-impl` —
  the Rust annotation macros.
- `rust/crates/specgate-discovery` — bindings, Rust/C# discovery, normalization.
- `rust/crates/specgate-ctsc` — registry/trace encoding, validation,
  comparison, and replay decoding.
- `rust/crates/specgate-cli` — the `specgate` executable.
- `csharp/` — `SpecGate.Annotations`, `SpecGate.Runtime`, `SpecGate.Weaver`,
  and the `SpecGate.Discovery` tool.
- [`test/behavior`](test/behavior/README.md) — behavior cases, native tests,
  shared expected bundles, and the golden matrix.
- `test/rejection`, `test/mechanics`, `test/support` — negative fixtures,
  product-mechanics tests, and shared C# test support.
- `docs/ctsc` — CTSC contracts, validator, and corpus.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md). Generated goldens under
`test/behavior/cases/<id>/expected/` are never hand-edited; regenerate them with
`just ctsc-goldens-update` (details in
[`CONTRIBUTING.md`](CONTRIBUTING.md#golden-matrix) and
[`test/behavior/README.md`](test/behavior/README.md)). Run `just check` before
committing and `just package-smoke` for release changes.
