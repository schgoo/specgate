# Capture a reference

## Goal

Run an implementation's tests under capture and publish a validated bundle:
the component's registry plus a reference trace of what the tests did.

## Prerequisites

- An instrumented implementation
  ([Rust](instrument-rust-crate.md) or [C#](instrument-csharp-assembly.md)).
- A binding ([`write-a-binding.md`](write-a-binding.md)).
- The tests pass under plain `cargo test` or `dotnet test`.

## Steps

1. Capture: [smoke]

   ```sh
   specgate capture --binding specgate.yaml --component <id> --impl <label> --out <capture-dir>
   ```

   - Omit `--impl` when the component has one implementation.
   - Omit `--out` to write to
     `.specgate/captures/<component>/<impl>/<UTC run id>/`.
   - Add `--force` to overwrite a non-empty `--out` directory.

2. Validate the bundle: [smoke]

   ```sh
   specgate validate bundle <capture-dir>
   ```

3. Commit the bundle if it is a reference you want to keep.

### What capture does

| Step | Rust | C# |
| --- | --- | --- |
| Discover the registry | Builds the package at `sources` and reads its annotations. | Runs `SpecGate.Discovery` over the sources project. |
| Build tests | `cargo test --no-run` in `sources` ([#96](https://github.com/schgoo/specgate/issues/96)). | `dotnet build` of each `tests.projects` entry with the weaver injected. |
| List tests | Each libtest binary's test list. | `dotnet test --list-tests`. |
| Run tests | One process per test, `--exact --test-threads=1`. | One `dotnet test` per test, filtered to that test. |
| Assemble | Every passing test that called an operation of the component becomes a scenario. | Same. |

- A failing test is skipped, not fatal. Capture fails only when no passing test
  produced a scenario for the component.
- Tests that pass but never call the component are skipped.
- A test that leaves an operation open when it ends is dropped with a
  diagnostic (see [`async-capture.md`](async-capture.md)).

## Expected output

[smoke]

```text
Complete(component=<id>, scenarios=<n>, operations=<n>, registry=<capture-dir>/registry.ctsc.json, trace=<capture-dir>/reference.otlp.json, manifest=<capture-dir>/manifest.json)
```

Exit code 0. On failure, `Error(<diagnostic>)` and exit code 1. These lines are
not yet a stable contract ([`../reference/cli.md`](../reference/cli.md#standard-output)).

## Artifacts

`manifest.json`, `registry.ctsc.json`, `reference.otlp.json`; see
[`../reference/bundle.md`](../reference/bundle.md).

## Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `is not empty; pass --force to overwrite it` | `--out` already has files. | Pick a new directory or pass `--force`. |
| `no passing tests captured operations for component` | No passing test called the component. | Fix the listed tests or add one that calls an operation. |
| `capture test build failed` | `cargo test --no-run` failed. | Run `cargo test` in `sources` and fix the build. |
| `the test host likely crashed before finishing` | A C# test process ended without a result. | Run that test alone with `dotnet test`. |

Full list: [`troubleshoot-capture.md`](troubleshoot-capture.md).

## Verify

`specgate validate bundle <capture-dir>` prints `valid: bundle <capture-dir>`
and exits 0. [smoke]

## Source of truth

- `rust/crates/specgate-cli/src/capture/` — `facade.rs`, `rust.rs`, `csharp.rs`,
  `encoding.rs`.
- [`../reference/bundle.md`](../reference/bundle.md)
- [`decisions/native-capture-finalization.md`](../decisions/native-capture-finalization.md)
- [`decisions/csharp-capture-cli.md`](../decisions/csharp-capture-cli.md)
