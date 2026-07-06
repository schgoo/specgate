# Binding files

A binding file connects a spec to a language-specific package. Specs
reference a binding by **path** — `binding: binding.yaml` — relative to
the spec file, or a list of paths for multi-implementation testing.

**Schema**: `binding-schema.json`.

## Structure

```yaml
# binding.yaml
language: rust            # required: "rust" or "csharp"
targets:
  default:
    package_root: ..      # path to the crate/project under test, relative to this file
```

All paths in a binding file are relative to the binding file's own
location.

## How specs reference bindings

Single binding:

```yaml
binding: binding.yaml
```

Multiple bindings (cross-implementation conformance):

```yaml
binding:
  - bindings/rust.yaml
  - bindings/csharp.yaml
```

When multiple bindings are listed, the harness runs all cases against
each binding and reports per-binding results.

## Targets

Target names correspond to operation names in the spec. The harness
resolves `operation: run_spec` in a case to the `run_spec` target in
the binding:

```yaml
targets:
  run_spec:
    package_root: ../rust/crates/specgate-harness
    function: specgate_harness::run_spec

  mechanism_proof:
    package_root: ../rust/crates/specgate-harness
    command: cargo test --test mechanism_proof
```

Target kinds:

| Field | Description |
|-------|-------------|
| `package_root` | Path to the crate/project, relative to this file |
| `function` | Fully qualified function to invoke |
| `command` | Shell command to run (exit 0 = pass) |
| `runtime` | Async runtime for driving async operations: `smol` (default) or `tokio` (Rust only) |

### Async runtime

For specs with `async: true` operations, the generated runner drives each
operation's future to completion with a single top-level runtime entry. The
`runtime` field on the target selects which runtime:

```yaml
targets:
  default:
    package_root: ../my-crate    # runtime omitted → smol (smol::block_on)
  tokio:
    package_root: ../my-crate
    runtime: tokio               # tokio current-thread runtime
```

- `smol` (the default) uses `smol::block_on` — a real executor with an
  on-demand reactor. Drives futures that resolve immediately as well as
  smol/async-std-style reactor futures.
- `tokio` establishes a current-thread tokio runtime. Use this when the
  implementation's futures require a tokio context (e.g. `tokio::time`,
  `tokio` I/O, or ecosystems built on tokio).

The runner and the crate under test must share the same tokio major version
(cargo unifies `tokio = "1"`), or a tokio future won't find its runtime
context. A spec selects a runtime by pointing its `target:` at the target that
declares the `runtime`.

## What belongs in bindings vs specs

| Concern | Where |
|---------|-------|
| What to test (operations, cases, expectations) | Spec |
| Which language to use | Binding |
| Where the package lives (`package_root`) | Binding |
| How to invoke (`command` / `function`) | Binding |
| Which async runtime drives async ops (`runtime`) | Binding |
| Which functions implement which operations | Source annotations |
