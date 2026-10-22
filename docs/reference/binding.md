# Binding reference (`specgate.yaml`)

A binding maps each component to the implementations that provide it. Every
command except `validate` and `compare` reads one. The schema is
[`binding-schema.json`](../../binding-schema.json); the design is
[`decisions/binding-schema-v1.md`](../decisions/binding-schema-v1.md). For a
walkthrough, see [`../guides/write-a-binding.md`](../guides/write-a-binding.md).

## Example

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
        csharp:
          framework: net10.0
```

The first line lets YAML editors validate the file. Adjust the relative path to
`binding-schema.json` for your layout. SpecGate itself ignores the comment.

## Fields

All paths are relative to the binding file. Unknown keys are rejected.

| Field | Required | Meaning |
| --- | --- | --- |
| `components` | yes | Map from component id to component. At least one entry. The key must equal the id in `#[spec_component("…")]` (Rust) or `[SpecOperation(…, Spec = "…")]` (C#). |
| `components.<id>.implementations` | yes | Map from implementation label to implementation. At least one entry. Labels are free-form and unique within the component; `--impl` selects one. |
| `…<label>.language` | yes | `rust` or `csharp`. |
| `…<label>.sources` | yes | Directory that discovery and replay build. Rust: the package directory containing `Cargo.toml`. C#: the directory whose first `.csproj` (by file name) is the sources project. |
| `…<label>.tests` | yes | Exactly one of `projects` or `command`. |
| `…<label>.tests.projects` | one of | Non-empty list of test project paths. See language notes below. |
| `…<label>.tests.command` | one of | A test command. Parsed but rejected by `capture` for every language ([#46](https://github.com/schgoo/specgate/issues/46)). |
| `…<label>.rust.runtime` | no | Replay engine: `auto` (default), `arty`, `smol`, or `tokio`. Automatic selection requires exactly one supported direct dependency. Only valid when `language: rust`. |
| `…<label>.rust.executor` | no | Crate-rooted custom replay wrapper, for example `crate::tests::replay_executor`. Cannot be combined with a runtime other than `auto`. |
| `…<label>.csharp.framework` | no | Target framework, such as `net10.0`. Only valid when `language: csharp`. |

A component that several projects declare is listed under each component id;
YAML anchors can share one implementation block, as in
[`../../test/behavior/specgate.yaml`](../../test/behavior/specgate.yaml).

## Current behavior by language

These describe what the current CLI does, including known gaps.

### Rust

- `capture` builds and runs the tests of the package at `sources`
  (`cargo test` in that directory). It does **not** read `tests.projects`; the
  field is still required by the schema
  ([#96](https://github.com/schgoo/specgate/issues/96)).
- Replay selects the candidate's own Arty, Smol, or Tokio dependency, including
  renamed and development dependencies. `rust.runtime` resolves ambiguity;
  no engine is guessed when several match. Capture retains the original tests'
  runtime configuration and does not use this field.
  Generated engine dependencies preserve local paths or exact registry
  versions; use `rust.executor` for other sources, such as Git.
- `rust.executor` replaces the built-in wrapper with candidate code, surrounding
  both setup construction and operation execution. The wrapper takes a
  `FnOnce() -> Future<Output = Result<(), String>>` task factory, constructs the
  future inside the configured execution context, and returns
  `Result<(), String>`. The factory may be `Send`; the future need not be.
  The function must be accessible from the selected generated test entry
  (`pub(crate)` with accessible parent modules suffices for a test target).
  See [custom replay execution](../guides/replay-a-candidate.md#runtime-selection-and-custom-configuration).
- Synchronous replay does not need an executor unless a runtime or custom
  wrapper is explicitly selected. `specgate-runtime` depends on no async engine.
- `capture` fails with "is not a Rust package (no Cargo.toml at …)" when
  `sources` has no `Cargo.toml`.

### C#

- `capture` requires `tests.projects`. Each listed project is built once with
  the capture weaver injected and each listed test runs as one scenario.
- `csharp.framework` is used by `capture` to pick one framework when a tests
  project targets several; without it, a multi-targeting tests project is
  rejected. `discover` does not read it.
- The test framework (xUnit, MSTest, or NUnit) is detected from the tests
  project's package references; there is no binding field for it.

## Selection rules

- `--component` must name a key under `components`.
- `--impl` must name a label under that component, and is required only when the component has several implementations;
  otherwise the single implementation is used.
- `replay` takes the component from the bundle and the implementation from
  `--impl`.

## Validation

The binding loader rejects, among others:

- a file with no components or an implementation with no fields it requires;
- unknown keys at any level;
- a `rust` block on a `csharp` implementation, or the reverse;
- a `tests` block with both or neither of `projects` and `command`.

A test in `specgate-discovery` checks every tracked `specgate*.yaml` and every
documentation YAML block that starts with the `$schema` comment above against
both the schema and the loader.

## Source of truth

- [`binding-schema.json`](../../binding-schema.json)
- `rust/crates/specgate-discovery/src/binding.rs` — loader and resolution.
- `rust/crates/specgate-cli/src/capture/rust.rs`, `capture/csharp.rs` — how
  capture uses each field.
