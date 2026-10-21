# Component-to-implementation binding

> **Status:** accepted
> **Owner:** user-ratified (schgoo)
> **Date:** 2026-10-16

## Context

A binding statically maps each component to the code that implements it, so a
caller names a component and need not know which project holds it. A
component is a bundle of operations declared in code (one `spec_component!` per
Rust crate); a registry document may hold several components
(`docs/ctsc/registry.md` section 2). Differential testing needs two
implementations of the same component. The current schema has a file-wide
`language` and named `targets`; `command` and `outputs` are never read,
`framework` is unread, and nothing says where C# tests live (#46).

## Decision

```yaml
components:
  orders.core:
    implementations:
      rust:
        language: rust
        sources: ../rust/crates/orders
        tests: { projects: [../rust/crates/orders] }
        rust: { runtime: tokio }
      csharp:
        language: csharp
        sources: ../csharp/Orders
        tests: { projects: [../csharp/Orders.Tests] }
        csharp: { framework: net10.0 }
```

- There is no `version` field; the shape is identified by `components`, and
  files in the earlier format are rejected.
- Each key under `components` is the exact `spec_component` name. A project
  that declares several components is listed under each; discovery verifies
  the selected project declares the component and fails on a mismatch.
- Each component lists named implementations. The label is unique within the
  component, and a component may have several implementations per language
  (for example versions).
- `sources` is what discovery and replay build. `tests` is what capture runs,
  in exactly one form:
  - `projects`: SpecGate builds, enumerates, and runs each test in its own
    process.
  - `command`: SpecGate runs the command with `SPECGATE_NATIVE_CAPTURE` set and
    bundles the runtime's sidecars.
- Language blocks (`rust: {runtime}`, `csharp: {framework}`) are selected by
  `language`; wrong-language fields are rejected.
- Paths are relative to the binding file, so bindings may live in different
  repositories.
- Unknown fields and empty or duplicate component ids or labels are rejected.
- CLI: `--binding <path>` defaults to `./specgate.yaml` and replaces the
  positional binding; `--component` and `--impl <label>` select the
  implementation. `--impl` is optional and auto-selected when the component
  has exactly one implementation; otherwise the error lists the labels. An
  empty `--impl ""` is rejected.
  `discover` and `capture` require `--component` (one bundle per capture call).
  `replay <capture-dir> --impl <label>` takes the component from the bundle.
- `package_root`, `targets`, `outputs`, the old `command`, the file-wide
  `language`, and `--target` are removed with no compatibility path.
- `tests.projects` and `tests.command` are parsed and validated now. Capture
  builds Rust `sources` and runs C# `tests.projects` through `dotnet`
  (`docs/decisions/csharp-capture-cli.md`), and rejects `tests.command` as not
  yet supported (#46). C# discovery and the weave target use `sources`; the
  build, enumeration, and run use `tests.projects`.
- `capture --out` is optional. The default is
  `./.specgate/captures/<component>/<impl>/<UTC timestamp>/` (for example
  `20261016T153100Z`), with the timestamp read through a mockable clock.
  Capture refuses a non-empty output directory unless `--force` is given; the
  timestamped default never collides. `.specgate/` is gitignored.
- Artifact label (user ratified): the capture manifest `target.name` and the
  trace attribute `conformance.target.name` equal the output directory's final
  path component, so the label matches the output (the run id for the default
  path). Replay keeps `--out` optional: the default is
  `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`, it refuses an
  existing path (including an empty directory) unless `--force`, and the candidate identity label is the out
  file name with `.json` then `.otlp` stripped (an empty label is rejected);
  replay refuses any existing out path, including an empty directory, unless
  `--force`. CTSC document shapes are unchanged. The harness uses fixed output
  directories ending in the implementation label. C# goldens are unchanged;
  Rust goldens were regenerated (118 files) with `target.name` `rust` where it
  was `default`, and the digests that cover those bytes changed accordingly.
- The golden matrix uses one shared `test/behavior/specgate.yaml`; rows name a
  component and an implementation label instead of a binding file.

## Deferred

- An optional `ref` on an implementation that makes SpecGate create a git
  worktree. It is the next planned slice; it is backward compatible and needs
  mockable I/O and cleanup.
- The command form needs scenario boundaries inside one process; today one
  process is one test.
- Whether one registry document per component should replace multi-component
  documents (`docs/component-identity-and-replay.md`, question 3).
