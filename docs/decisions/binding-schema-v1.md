# Component-to-implementation binding (version 1)

> **Status:** proposed
> **Owner:** SpecGate maintainers (awaiting final ratification)
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
version: 1
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

- `version` is required.
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
- CLI: `--binding <path>` defaults to `./specgate.yaml`; `--component` and
  `--impl <label>` select the implementation.
- `package_root`, `targets`, `outputs`, and the file-wide `language` are
  removed with no compatibility path.

## Deferred

- An optional `ref` on an implementation that makes SpecGate create a git
  worktree. It is backward compatible and needs mockable I/O and cleanup.
- The command form needs scenario boundaries inside one process; today one
  process is one test.
- Whether one registry document per component should replace multi-component
  documents (`docs/component-identity-and-replay.md`, question 3).
- Migration of `test/bindings/*.yaml`, `binding.rs`, the CLI, docs, and
  goldens.
