# CLI reference

`specgate` is a single executable with five commands: `discover`, `capture`,
`replay`, `validate`, and `compare`. Install it with
[`../guides/install.md`](../guides/install.md).

## Usage

`specgate --help` (or `-h`) prints this text to stderr and exits 0. A test in
`specgate-cli` keeps the block below identical to the program's output.

<!-- specgate-usage:start -->
```text
usage: specgate <command> [options] <args>

commands:
  discover --component <id> [--binding <specgate.yaml>] [--impl <label>] --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json>
  capture --component <id> [--binding <specgate.yaml>] [--impl <label>] [--out <dir>] [--force]
  replay <capture-dir> [--binding <specgate.yaml>] [--impl <label>] [--out <candidate.otlp.json>] [--force]
  validate registry <registry.json> [--import <registry.json>]...
  validate trace <trace.otlp.json|trace.otlp.jsonl>
  validate linked <trace> <root-registry> [--import <registry.json>]...
  validate bundle <capture-dir>
  compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...
```
<!-- specgate-usage:end -->

## Common options

| Option | Meaning |
| --- | --- |
| `--binding <path>` | Binding file. Defaults to `./specgate.yaml`. See [`binding.md`](binding.md). |
| `--component <id>` | Component id as declared in the binding and in `#[spec_component]` or `Spec =`. |
| `--impl <label>` | Implementation label under `components.<id>.implementations`. Required only when the component has several implementations. |
| `--force` | Overwrite existing output. |

## Commands

`discover`, `capture`, and `replay` exclusively own the selected package from
discovery through output publication. An overlapping command targeting the
same canonical package sources fails immediately with exit code 1 and
`package already in use by another SpecGate command`; retry after the first
command finishes. Different packages remain independent, and ordinary
`cargo test` parallelism is unchanged.

### `discover`

Builds the implementation and writes its CTSC registry (`registry.ctsc.json`)
for the component and its dependencies. `--registry-id`, `--registry-version`,
and `--out` are required.

### `capture`

Builds the implementation's tests, runs each test as one scenario, and writes a
capture bundle. See [`../guides/capture-a-reference.md`](../guides/capture-a-reference.md)
and [`bundle.md`](bundle.md).

- `--out` defaults to `./.specgate/captures/<component>/<impl>/<UTC run id>/`.
- A non-empty output directory requires `--force`.
- The bundle's `target.name` is the output directory's name.

### `replay`

Replays the bundle's recorded stimuli against a candidate implementation and
writes a candidate trace. The component comes from the bundle. Candidates must
be Rust implementations. See
[`../guides/replay-a-candidate.md`](../guides/replay-a-candidate.md).

- `--out` defaults to `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`.
- An existing output file requires `--force`.

### `validate`

Validates one artifact at one level:

| Level | Checks |
| --- | --- |
| `registry` | A registry document, with optional `--import` registries. |
| `trace` | Trace Core rules for a JSON or JSONL trace. No `--import`. |
| `linked` | A trace against its root registry, with optional `--import` registries. |
| `bundle` | A capture directory: manifest, digests, trace, and linkage. No `--import`. |

### `compare`

Compares a reference trace with a candidate trace using the fixed
`ctsc.strict/0.2.0` policy ([`../ctsc/comparison.md`](../ctsc/comparison.md)).
`--registry` may appear at most once; `--import` requires `--registry`.
Scenarios pair by name.

## Exit codes

Observed behavior:

| Code | Meaning |
| --- | --- |
| 0 | Success; `validate` found the artifact valid; `compare` found the traces equivalent; `--help`. |
| 1 | The operation failed; `validate` found the artifact invalid; `compare` found a mismatch. |
| 2 | Usage error: no arguments, unknown command, or invalid arguments. |

An unknown command prints `error: unknown command '<name>'` followed by the
usage text. Other argument errors print `error: <message>`. All of these go to
stderr.

## Standard output

> **Not yet a stable contract; may change.** The lines below describe current
> output so you can read it. Do not parse them in automation; use exit codes and
> the written artifacts instead.

| Command | Success | Failure |
| --- | --- | --- |
| `discover` | `Complete(component=…, operations=…, types=…, output=…)` | `Error(<diagnostic>)` |
| `capture` | `Complete(component=…, scenarios=…, operations=…, registry=…, trace=…, manifest=…)` | `Error(<diagnostic>)` |
| `replay` | `Complete(component=…, scenarios=…, operations=…, plans=…, output=…)` | `Error(<diagnostic>)` |
| `validate` | `valid: <level> <artifact>` | `invalid: <level> <artifact>`, then one `  <location>: <message>` line per finding |
| `compare` | `policy: <name>/<version>`, `reference: <path>`, `candidate: <path>`, `equivalent: true` | The same header with `equivalent: false`, then `validation …`, `error …`, or `mismatch <path>: expected <x>, actual <y>` lines |

A successful `capture` also prints one `warning: <scenario>: <message>` line to
stderr for each test it dropped because an operation observed a value that
cannot be recorded, such as a null in a non-nullable C# position. The message
names the operation and position and asks whether the position should be
declared nullable (`optional<T>` in the registry).

Example `compare` mismatch output:

```text
policy: ctsc.strict/0.2.0
reference: rust/reference.otlp.json
candidate: csharp/reference.otlp.json
equivalent: false
mismatch scenario["stateless::add_two_and_three"]: expected present, actual <missing>
```

## Environment variables

| Variable | Effect |
| --- | --- |
| `SPECGATE_DISCOVERY_PATH` | Directory or `SpecGate.Discovery.dll` used for C# discovery. A relative value resolves against the working directory. Defaults to the directory of the `specgate` executable. |
| `SPECGATE_WEAVER_PATH` | Directory holding the C# capture weaver, runtime, and `SpecGateCapture.targets`. Defaults to the directory of the `specgate` executable. |
| `SPECGATE_CACHE_DIR` | Root for scratch build directories. Defaults to `%LOCALAPPDATA%\SpecGate\Cache` on Windows, otherwise `$XDG_CACHE_HOME`, then `$HOME/.cache`. |
| `SPECGATE_RUNTIME_PATH`, `SPECGATE_RUNTIME_VERSION` | Advanced. Assert the exact `specgate-runtime` package a replay runner uses; a mismatch with the candidate's own resolved package fails. They never substitute a different runtime. |

`SPECGATE_NATIVE_CAPTURE` is set by `capture` for each test process. It is
internal; do not set it yourself.

## Source of truth

- `rust/crates/specgate-cli/src/main.rs` and `src/main/` — argument parsing,
  output, and exit codes.
- `rust/crates/specgate-cli/src/capture/`, `src/replay/` — command behavior.
