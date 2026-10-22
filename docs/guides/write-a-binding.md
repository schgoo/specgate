# Write a binding

## Goal

Write the `specgate.yaml` that tells `specgate` where each implementation of a
component lives and how to run its tests.

## Prerequisites

- An instrumented implementation
  ([Rust](instrument-rust-crate.md) or [C#](instrument-csharp-assembly.md)).
- Its component id: the string in `spec_component!("…")` /
  `#[spec_operation(…, spec = "…")]` or `[SpecOperation(…, Spec = "…")]`.

## Steps

### 1. Rust implementation

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
  [runtime configuration](replay-a-candidate.md#runtime-selection-and-custom-configuration).

### 2. C# implementation

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

### 3. Both implementations of one component

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

### 4. Paths and location

- Paths are relative to the binding file.
- Commands read `./specgate.yaml` unless you pass `--binding <path>`.
- Keep the `# yaml-language-server` line and adjust its relative path so your
  editor validates the file against
  [`binding-schema.json`](../../binding-schema.json).

## Expected output

Any command that loads the binding succeeds, for example:

```sh
specgate discover --component example.math --registry-id urn:ctsc:registry:example --registry-version 0.1.0 --out registry.ctsc.json
```

prints `Complete(component=example.math, …)` and exits 0. [smoke]

## Artifacts

`specgate.yaml`, which you commit next to your code.

## Failure modes

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

## Verify

Run `discover` as above, or a capture
([`capture-a-reference.md`](capture-a-reference.md)). Both load and validate the
binding first. [smoke]

## Source of truth

- [`../reference/binding.md`](../reference/binding.md) — every field.
- [`binding-schema.json`](../../binding-schema.json)
- `rust/crates/specgate-discovery/src/binding.rs`
- [`decisions/binding-schema-v1.md`](../decisions/binding-schema-v1.md)
