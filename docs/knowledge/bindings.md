# Binding files

Binding files connect a spec to a language-specific implementation. They tell
the harness what language backend to use and where the project lives.

**File convention**: `bindings/<name>.yaml`
**Schema**: `binding-schema.json`

## Structure

```yaml
language: rust    # required: rust | csharp
project_root: packages/my-component   # path to the implementation project
```

## How specs reference bindings

The spec declares `binding: rust`. The harness resolves this to
`bindings/rust.yaml`, reads the `language` field, and selects the
corresponding codegen backend.

```yaml
# specs/parser.spec.yaml
name: parser
binding: rust

# bindings/rust.yaml
language: rust
project_root: packages/parser
```

## How the harness resolves spec names to code

The binding does not declare which functions to call. Instead:

1. **Annotations** in the source code register metadata into a compile-time
   registry (spec name, kind, code path)
2. **Build** — the harness builds the project with the `specgate` feature,
   which causes proc macros to populate the registry
3. **Discover** — the harness reads the registry to get a name → symbol map
4. **Generate** — the harness produces test code using resolved symbols
5. **Test** — `cargo test` runs the generated tests

The spec references operations by name (e.g. `operation: cache_get`), and the
annotation `#[spec_operation(name = "cache_get")]` on the actual function
provides the mapping. No manual wiring is needed in the binding.

## When to create a binding

Create a binding when implementing a spec for a specific language.
Each language gets its own binding file. The spec stays language-agnostic.

## What belongs in bindings vs specs

| Concern | Where |
|---------|-------|
| What behavior to test | Spec |
| What types/inputs/outputs | Spec |
| What language to use | Binding |
| Where the project lives | Binding |
| Which functions implement which spec names | Annotations (in source code) |
