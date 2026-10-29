---
name: specgate-capture
description: "Writes a specgate.yaml binding and runs specgate discover and specgate capture to record reference behavior from the user's existing Rust or C# tests into a CTSC bundle. Use when declaring components in specgate.yaml, recording a reference, or reading capture output."
---

# Bind and capture with SpecGate

Part of the SpecGate agent plugin 0.6.0. It covers using SpecGate in the user's project, not developing SpecGate itself.

## Path resolution

`${PLUGIN_DIR}` is the plugin root, the directory that holds `.claude-plugin/plugin.json`.

- **Claude Code:** `${PLUGIN_DIR}` is `${CLAUDE_PLUGIN_ROOT}`.
- **Other agents:** this file is `skills/specgate-capture/SKILL.md`, so the plugin root is two directories up.

`${PLUGIN_DIR}/docs/` holds every SpecGate guide and reference page. Links to `https://github.com/schgoo/specgate/…/v0.6.0/` show SpecGate's own sources and examples at this release. Read them as examples; do not adopt SpecGate's contributor workflow (`just` recipes, the golden matrix) in the user's project.

## Write a binding

### Goal

Write the `specgate.yaml` that tells `specgate` where each implementation of a
component lives and how to run its tests.

### Prerequisites

- An instrumented implementation
  ([Rust](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md) or [C#](${PLUGIN_DIR}/docs/guides/instrument-csharp-assembly.md)).
- Its component id: the string in `spec_component!("…")` /
  `#[spec_operation(…, spec = "…")]` or `[SpecOperation(…, Spec = "…")]`.

### Steps

#### 1. Rust implementation

```yaml
# yaml-language-server: $schema=../../binding-schema.json
components:
  example.math:
    implementations:
      rust:
        language: rust
        sources: crates/math
        tests:
          projects:
            - crates/math/Cargo.toml
```

- `sources` is the package directory holding `Cargo.toml`. Capture runs
  `cargo test` there.
- `tests.projects` is required by the schema but **not read** by Rust capture
  today; list the same package
  ([#96](https://github.com/schgoo/specgate/issues/96)).
- Replay automatically selects the only supported direct engine dependency:
  Arty, Smol, or Tokio. With several, set `rust: { runtime: arty }` (or `smol`
  or `tokio`). Capture still uses your ordinary tests' runtime configuration.
- For custom configuration, use
  `rust: { executor: "crate::tests::replay_executor" }`; the function owns setup
  and async execution. See
  [runtime configuration](${PLUGIN_DIR}/docs/guides/replay-a-candidate.md#runtime-selection-and-custom-configuration).

#### 2. C# implementation

```yaml
# yaml-language-server: $schema=../../binding-schema.json
components:
  fixture.cli_capture.add:
    implementations:
      csharp:
        language: csharp
        sources: sources
        tests:
          projects:
            - tests/SpecGate.CliCaptureFixtures.Tests.csproj
```

- `sources` is a directory; its first `.csproj` by file name is the project
  that gets woven.
- Each `tests.projects` entry is built and its tests are captured.
- Add `csharp: { framework: net10.0 }` when a tests project targets several
  frameworks.

#### 3. Both implementations of one component

```yaml
# yaml-language-server: $schema=../../binding-schema.json
components:
  calc.add:
    implementations:
      rust:
        language: rust
        sources: rust/calc
        tests:
          projects:
            - rust/calc/Cargo.toml
      csharp:
        language: csharp
        sources: csharp/Calc
        tests:
          projects:
            - csharp/Calc.Tests/Calc.Tests.csproj
```

With several implementations, every command that reads the binding needs
`--impl rust` or `--impl csharp`.

#### 4. Paths and location

- Paths are relative to the binding file.
- Commands read `./specgate.yaml` unless you pass `--binding <path>`.
- Keep the `# yaml-language-server` line and adjust its relative path so your
  editor validates the file against
  [`binding-schema.json`](${PLUGIN_DIR}/binding-schema.json).

### Expected output

Any command that loads the binding succeeds, for example:

```sh
specgate discover --component example.math --registry-id urn:ctsc:registry:example --registry-version 0.1.0 --out registry.ctsc.json
```

prints `Complete(component=example.math, …)` and exits 0.

### Artifacts

`specgate.yaml`, which you commit next to your code.

### Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `not found in binding` | `--component` does not match a key. | Match the key exactly; the message lists the available components. |
| `not found for component` | `--impl` does not match a label. | Use a listed label. |
| `has several implementations; select one by label` | `--impl` is missing. | Pass `--impl <label>`. |
| `tests must declare exactly one of projects or command` | Both or neither present. | Use `projects`. |
| `which is not yet supported (#46)` | `tests.command` is set. | Use `tests.projects` ([#46](https://github.com/schgoo/specgate/issues/46)). |
| `C# capture requires tests.projects` | A C# implementation without test projects. | Add `tests.projects`. |
| `targets several frameworks; set csharp.framework in the binding to choose one` | A multi-targeting tests project. | Add `csharp.framework`. |

Unknown keys and language blocks that do not match `language` are also
rejected.

### Verify

Run `discover` as above, or a capture
([`capture-a-reference.md`](${PLUGIN_DIR}/docs/guides/capture-a-reference.md)). Both load and validate the
binding first.

## Capture a reference

### Goal

Run an implementation's tests under capture and publish a validated bundle:
the component's registry plus a reference trace of what the tests did.

### Prerequisites

- An instrumented implementation
  ([Rust](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md) or [C#](${PLUGIN_DIR}/docs/guides/instrument-csharp-assembly.md)).
- A binding ([`write-a-binding.md`](${PLUGIN_DIR}/docs/guides/write-a-binding.md)).
- The tests pass under plain `cargo test` or `dotnet test`.

### Steps

1. Capture:

   ```sh
   specgate capture --binding specgate.yaml --component <id> --impl <label> --out <capture-dir>
   ```

   - Omit `--impl` when the component has one implementation.
   - Omit `--out` to write to
     `.specgate/captures/<component>/<impl>/<UTC run id>/`.
   - Add `--force` to overwrite a non-empty `--out` directory.

2. Validate the bundle:

   ```sh
   specgate validate bundle <capture-dir>
   ```

3. Commit the bundle if it is a reference you want to keep.

#### What capture does

| Step | Rust | C# |
| --- | --- | --- |
| Discover the registry | Builds the package at `sources` and reads its annotations. | Runs `SpecGate.Discovery` over the sources project. |
| Build tests | `cargo test --no-run` in `sources` ([#96](https://github.com/schgoo/specgate/issues/96)). | `dotnet build` of each `tests.projects` entry with the weaver injected. |
| List tests | Each libtest binary's test list. | `dotnet test --list-tests`. |
| Run tests | One process per test, `--exact --test-threads=1`. | One `dotnet test` per test, filtered to that test. |
| Assemble | Every passing test with a top-level operation of the selected component becomes a scenario. Those top-level subtrees are kept, including nested calls to other components; a call reached only beneath another component's operation produces no scenario. | Same. |

- A failing test is skipped, not fatal. Capture fails only when no passing test
  produced a scenario for the component.
- Tests that pass but never call the component are skipped.
- A test that leaves an operation open when it ends is dropped with a
  diagnostic (see [`async-capture.md`](${PLUGIN_DIR}/docs/guides/async-capture.md)).

### Expected output

```text
Complete(component=<id>, scenarios=<n>, operations=<n>, registry=<capture-dir>/registry.ctsc.json, trace=<capture-dir>/reference.otlp.json, manifest=<capture-dir>/manifest.json)
```

Exit code 0. On failure, `Error(<diagnostic>)` and exit code 1. These lines are
not yet a stable contract ([`../reference/cli.md`](${PLUGIN_DIR}/docs/reference/cli.md#standard-output)).

### Artifacts

`manifest.json`, `registry.ctsc.json`, `reference.otlp.json`; see
[`../reference/bundle.md`](${PLUGIN_DIR}/docs/reference/bundle.md).

### Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `is not empty; pass --force to overwrite it` | `--out` already has files. | Pick a new directory or pass `--force`. |
| `no passing tests captured operations for component` | No passing test called the component. | Fix the listed tests or add one that calls an operation. |
| `capture test build failed` | `cargo test --no-run` failed. | Run `cargo test` in `sources` and fix the build. |
| `the test host likely crashed before finishing` | A C# test process ended without a result. | Run that test alone with `dotnet test`. |

Full list: [`troubleshoot-capture.md`](${PLUGIN_DIR}/docs/guides/troubleshoot-capture.md).

### Verify

`specgate validate bundle <capture-dir>` prints `valid: bundle <capture-dir>`
and exits 0.
