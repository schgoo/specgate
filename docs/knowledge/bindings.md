# Binding files

Binding files connect a spec to a language-specific implementation. They tell
the harness what language backend to use and where targets live.

**File convention**: `bindings/<name>.yaml`
**Schema**: `binding-schema.json`

## Structure

```yaml
language: rust    # required: rust | csharp

targets:
  test-geometry:
    package_root: ../rust/crates/geometry
    test_root: ../rust/crates/geometry/tests
  test-renderer:
    package_root: ../rust/crates/renderer
    command: cargo run -p renderer -- {input}
```

All paths are relative to the binding file location.

## How specs reference bindings

The spec declares a binding name and target:

```yaml
name: geometry.area
binding:
  name: rust
  target: test-geometry
```

The harness resolves `rust` to `bindings/rust.yaml`, finds the `test-geometry`
target, and uses it to determine how to build, generate tests, and run them.

## Target types

See `docs/knowledge/targets.md` for details on command, API, and
build-only targets.

## When to create a binding

Create a binding when implementing a spec for a specific language.
Each language gets its own binding file. The spec stays language-agnostic.

## What belongs in bindings vs specs

| Concern | Where |
|---------|-------|
| What behavior to test | Spec |
| What types/inputs/outputs | Spec |
| What language to use | Binding |
| Where the project lives | Binding (target `package_root`) |
| How to run tests | Binding (target `command` or `function`) |
| Which functions implement which spec names | Annotations (in source code) |
