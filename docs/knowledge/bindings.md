# Binding files

Binding files are language-specific build metadata. They tell the harness how
to execute targets, deliver inputs, and read outputs.

**File convention**: `bindings/<language>.yaml`
**Schema**: `binding-schema.json`

## Structure

```yaml
language: rust    # required: rust | csharp
targets:          # required: named targets referenced by specs
  <name>:         # arbitrary name — specs reference this via target: <name>
    ...
```

## Target types

### Command targets — run a shell command

```yaml
targets:
  build:
    command: cargo build -p fixture --message-format=json
    inputs:
      source:
        file: "{workdir}/fixture/src/lib.rs"
    outputs:
      file: "{workdir}/target/specgate-annotations.json"
      stderr: true
```

### Call targets — invoke a function directly

```yaml
targets:
  validate:
    call: specgate_core::validate
```

For call targets, spec inputs map directly to function parameters by name.
Outputs come from the return value unless overridden.

## Input delivery (command targets only)

Each spec input name maps to a delivery mechanism. The key name is the mechanism:

| Key | Meaning | Value |
|-----|---------|-------|
| `file` | Write input value to a file | File path (supports `{workdir}`) |
| `env` | Set an environment variable | Env var name |
| `arg` | Pass as a CLI flag | Flag name (e.g. `--threshold`) |
| `positional` | Pass as positional argument | Index (0-based) |

```yaml
inputs:
  source:
    file: "{workdir}/fixture/src/lib.rs"   # writes the value to this file
  mode:
    env: SPECGATE_MODE                      # sets this env var
  threshold:
    arg: --threshold                        # passes as --threshold <value>
```

## Output reading

| Field | Meaning |
|-------|---------|
| `file` | Read output JSON from a file path |
| `stdout` | Parse stdout as JSON |
| `stderr` | Parse stderr for error outputs |
| `exit_code` | Map exit code to a spec output field name |

## How specs reference bindings

The spec says `target: build`. The harness resolves the binding first:
`binding: rust` → `bindings/rust.yaml` → look up `targets.build`.

```yaml
# specs/rust.annotations.spec.yaml
name: rust.annotations
binding: rust
target: build

# bindings/rust.yaml
targets:
  build:
    command: cargo build -p fixture
    inputs:
      source:
        file: "{workdir}/fixture/src/lib.rs"
    outputs:
      file: "{workdir}/target/specgate-annotations.json"
```

## When to create a binding

Create a binding when implementing a spec for a specific language.
Each language gets its own binding file. The spec stays language-agnostic.

## What belongs in bindings vs specs

| Concern | Where |
|---------|-------|
| What behavior to test | Spec |
| What types/inputs/outputs | Spec |
| How to build/execute | Binding |
| How inputs are delivered | Binding |
| Where output files are | Binding |
| Language-specific toolchain config | Binding |
