# Specgate-Cli

[![crates.io](https://img.shields.io/crates/v/specgate-cli.svg)](https://crates.io/crates/specgate-cli)
[![docs.rs](https://docs.rs/specgate-cli/badge.svg)](https://docs.rs/specgate-cli)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Command-line interface for `SpecGate`’s CTSC-native workflow.

```text
specgate discover --component <id> [--binding <specgate.yaml>] [--impl <label>] --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json>
specgate capture --component <id> [--binding <specgate.yaml>] [--impl <label>] [--out <dir>] [--force]
specgate replay <capture-dir> [--binding <specgate.yaml>] [--impl <label>] [--out <candidate.otlp.json>] [--force]
specgate validate registry <registry.json> [--import <registry.json>]...
specgate validate trace <trace.otlp.json|trace.otlp.jsonl>
specgate validate linked <trace> <root-registry> [--import <registry.json>]...
specgate validate bundle <capture-dir>
specgate compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...
```

`discover` exports a deterministic CTSC registry from Rust link-time or C#
compiled-assembly metadata. `capture` runs ordinary Rust or C# tests in isolation
and records the selected component’s top-level operation subtrees from
passing native scenarios, keeping nested calls into other annotated
components verbatim. `replay` verifies a capture bundle,
statically links its top-level semantic inputs to a Rust candidate, and
emits an independent deterministic CTSC trace. `validate` provides native
CTSC 0.3 Registry, Trace Core, Linked, and capture-bundle validation.
`compare` applies deterministic `ctsc.strict/0.1.0` semantics. No command
reads `.spec.yaml` or invokes Python. The library capture entry point accepts
one owned [`CaptureRequest`][__link0], constructed with a validated builder, and
returns `Result<CaptureReport, CaptureError>`.

The [CLI reference][__link1]
holds the exact usage text, exit codes, and output lines; its usage block is
tested against the binary.

Without `--out`, `capture` writes to
`./.specgate/captures/<component>/<impl>/<UTC run id>/` (for example
`20261016T153100Z`) and refuses a non-empty output directory unless
`--force` is given. The output directory’s final name is recorded as the
artifact `target.name` in the manifest and trace. `replay` without `--out`
writes `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`,
refuses any existing output path unless `--force`, and labels the candidate target
with the output file name minus its `.otlp.json` suffix.

## Examples

```rust
use specgate_cli::{CapturePaths, CaptureRequest, capture};

let request = CaptureRequest::builder(CapturePaths {
    binding: "specgate.yaml".into(),
})
    .component("example.math")
    .out("capture")
    .build()?;
let report = capture(request)?;
println!("registry: {}", report.registry_path.display());
```


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__cargo_doc2readme_dependencies_info]: ggGmYW0CYXZlMC43LjNhdIQbJSusbBjLO7EbSlASCvKRTqwbmd2gsLYkxMobU3WiDiuhvKthYvRhcoQbmcDD44vczCQbj2gPsMcXSuIbE7b_AVKIiggbfKMWcXwOhUthZIGDbHNwZWNnYXRlLWNsaWUwLjYuMGxzcGVjZ2F0ZV9jbGk
 [__link0]: https://docs.rs/specgate-cli/0.6.0/specgate_cli/?search=CaptureRequest
 [__link1]: https://github.com/schgoo/specgate/blob/main/docs/reference/cli.md
