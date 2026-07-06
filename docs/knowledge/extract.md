# Extracting a spec from code

`specgate extract` runs the workflow **in reverse**: instead of implementing a
spec, it *derives* a `.spec.yaml` from an already-annotated crate. Use it to
bootstrap a spec for existing annotated code, or to keep a spec in sync with an
implementation.

```bash
specgate extract <package-root> -o|--out <spec.yaml> [--component <name>] [--cases]
```

| Argument / flag | Required | Meaning |
|-----------------|----------|---------|
| `<package-root>` | yes | Path to the annotated Rust crate to extract from |
| `-o`, `--out <spec.yaml>` | yes | Output path for the generated spec |
| `--component <name>` | no | Which component to extract (required when the crate hosts more than one) |
| `--cases` | no | Also capture test cases by running the crate's existing tests |

## What it produces

Extraction writes two files:

1. The `<spec>.spec.yaml` at the `-o` path.
2. A **sibling binding** file next to it, named `<stem>.binding.yaml` (the spec
   file name with its `.spec.yaml` / `.yaml` suffix removed). Its
   `package_root` is written relative to the binding's own location.

## Schema by default

Without `--cases`, extraction derives **only the schema** — operations (with
their inputs/outputs), and types — leaving `cases:` empty. A freshly-extracted
skeleton therefore validates as sound except for the expected `no_cases`
finding. This is the fast path for scaffolding a spec you will then fill in.

## `--cases` — capture runnable cases

With `--cases`, the crate's **existing tests are run under record mode**, and
each passing test is captured as a concrete case (inputs + expected trace).
The result is a complete, runnable spec.

## How it works (deterministic, no LLM)

Extraction reads the crate's operation/type registry rather than parsing source
or invoking an LLM. A small discovery binary is scaffolded that depends on the
target crate, reads the registry the annotations register at link time, and
prints it as JSON; that JSON is mapped to the spec skeleton and binding. The
output is deterministic — the same crate produces byte-identical files, which
is what the `extract-check` / `extract-update` golden tests rely on.

## Components and `depends_on`

Each annotated item belongs to a **component** declared with
`spec_component!` (see [annotations.md](annotations.md#spec_component--the-component-axis)).
When a crate hosts more than one component, `--component <name>` selects which
one to extract; extracting an ambiguous multi-component crate without a
selector is an error. When the selected component's operations reference a type
owned by another component, extraction derives a `depends_on:` edge and
references that type by bare name instead of redefining it.

## Reference

- CLI wiring: `rust/crates/specgate-cli/src/main.rs` (`cmd_extract`)
- Implementation: `rust/crates/specgate-cli/src/extract.rs`
- Golden fixtures and the extract list: `scripts/extract-goldens.ps1`
  (run via `just extract-check` / `just extract-update`)
