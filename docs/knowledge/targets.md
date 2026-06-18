# Binding targets

Each binding file defines one or more **targets**. A target tells the
harness how to test a particular spec against an implementation. The
simplest target — and the one every fixture uses — has only a
`package_root`:

```yaml
language: rust
targets:
  default:
    package_root: ..
```

This is enough for the harness to discover annotations in the crate at
`package_root`, generate tests, and run them.

## Target fields

| Field | Required | Description |
|-------|----------|-------------|
| `package_root` | yes | Path to the project root, relative to the binding file. Commands run from here. |
| `test_root` | no | Where to write generated test files, relative to the binding file. |
| `build` | no | Optional build command run before execution. |
| `command` | no | Shell command template for command targets (supports `{workdir}`, `{spec_path}`). |
| `function` | no | Fully qualified function for API targets. |
| `constructor` | no | Fully qualified constructor for API targets that need a receiver. |
| `outputs.file` | no | Path to an output file the harness reads. |
| `outputs.stdout` | no | Format to parse stdout in (e.g. `"json"`). |

See `binding-schema.json` for the authoritative definitions.

## Target shapes

### Default (annotation discovery)

```yaml
targets:
  default:
    package_root: ../my-crate
```

The harness builds the crate, discovers annotated symbols, generates
per-case tests, and runs them. This is what every fixture uses.

### Command target

```yaml
targets:
  run-cli:
    package_root: ../my-cli
    command: cargo run -q -p my-cli -- {spec_path}
    outputs:
      stdout: json
```

The harness invokes the command, reads stdout (or `outputs.file`),
parses it as JSON, and treats the result as the test outcome. Useful
for testing CLIs or external tools.

### API target

```yaml
targets:
  validate-spec:
    package_root: ../my-crate
    function: my_crate::validate
```

The harness calls the named function in-process. Useful when the
implementation is a library and you don't need a subprocess.

### Build-only target

```yaml
targets:
  compile-check:
    package_root: ../my-crate
    build: cargo build -q
```

A target with only `build` and no `command` / `function` — used when
"does it compile" is the entire assertion (e.g. trybuild-style negative
tests).

## String inputs

When a case input is a string (e.g. source code), the generated test
embeds it as a literal. For command targets the test may write it to a
temp file and pass that path through the command template; this is the
generator's responsibility, not the shell's.

## Error cases in command targets

For a case where the expected trace stream contains an error event
(e.g. `<op>.outcome: "Error"`), the command may exit non-zero or emit
an error payload in stdout. The harness compares trace events, not
exit codes — exit-code interpretation is a per-target detail to be
decided by the binding.
