# Binding files

A binding file connects a spec to a language-specific package. Specs
reference a binding by **path** — `binding: binding.yaml` — relative to
the spec file.

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

The simplest fixture binding (`test/rust/crates/specgate-fixtures/specs/binding.yaml`)
is exactly the example above — one `default` target pointing at the
fixture crate.

## How specs reference bindings

```yaml
# fixture.stateless_add.spec.yaml
name: fixture.stateless_add
binding: binding.yaml      # sibling file
cases:
  - name: add_2_3
    operation: add
    inputs: { a: 2, b: 3 }
    expected:
      - add.result: "5"
```

The harness reads `binding.yaml`, finds `language: rust`, locates the
`package_root`, discovers `#[spec_operation("add")]` in that crate, and
runs the case.

## Targets

A binding may declare multiple named targets — useful when one package
hosts several test contexts. Most fixtures only need `default`. For the
full target shape (`command`, `function`, `build`, `outputs`, …) see
[`targets.md`](targets.md) and `binding-schema.json`.

## What belongs in bindings vs specs

| Concern | Where |
|---------|-------|
| What to test (behaviour, cases, expectations) | Spec |
| Which language to use | Binding |
| Where the package lives (`package_root`) | Binding |
| How to invoke (`command` / `function`) | Binding (optional) |
| Which functions implement which names | Source annotations |
