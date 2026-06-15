# Binding targets

Each binding file defines one or more **targets**. A target tells the harness
how to test a particular spec against an implementation.

## Target types

### Command target

A target with a `command` field. The harness runs the command for each test
case, substituting `{input_name}` placeholders with values from the case
inputs. The command outputs JSON to stdout, which the harness parses and
compares against the spec's expected output.

```yaml
targets:
  test-annotations:
    package_root: ../rust/crates/geometry
    command: cargo run -p annotation-test-runner -- {source}
```

If the command references a tool that does not exist yet (e.g.
`annotation-test-runner`), **building that tool is part of the
implementation**. The implementation must ensure the tool exists and
produces the correct JSON output for each test case.

### API target

A target with a `function` field. The harness generates test code that
calls the function directly with case inputs and checks return values.

```yaml
targets:
  test-geometry:
    package_root: ../rust/crates/geometry
    test_root: ../rust/crates/geometry/tests
    function: geometry::compute_area
```

### Build-only target

A target with only `build` (no command or function).
Used for targets that just need to compile successfully.

## Annotations

Annotations are a discovery mechanism. Any target can
have annotated source code. The harness builds the project, reads
`annotations.json`, and uses the discovered symbols to generate test code
that exercises the annotated operations.

## Target fields

| Field | Required | Description |
|-------|----------|-------------|
| `package_root` | Yes | Path to the project root, relative to the binding file |
| `test_root` | No | Where to write generated tests, relative to the binding file |
| `command` | No | Shell command template for command targets |
| `function` | No | Fully qualified function for API targets |
| `constructor` | No | Constructor for API targets that need object creation |
| `build` | No | Custom build command |

## String inputs in command targets

When a case input is a string (e.g. source code), the generated test
embeds it as a string literal. The test code can write it to a temp file
and pass the file path to the command — the transport is handled by the
generated test, not the shell.

## Error cases in command targets

For test cases with `outcome: Error`, the command should exit with a
non-zero status or output JSON with an error structure. The harness
compares the error output against the spec's expected errors.

Language-specific details for error testing (e.g. Rust `trybuild` for
compile errors, C# Roslyn diagnostics) belong in the language knowledge
files, not here.
