# Specgate-Cli

[![crates.io](https://img.shields.io/crates/v/specgate-cli.svg)](https://crates.io/crates/specgate-cli)
[![docs.rs](https://docs.rs/specgate-cli/badge.svg)](https://docs.rs/specgate-cli)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Command-line interface for `SpecGate`’s CTSC-native workflow.

```text
specgate discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> --out <registry.ctsc.json> [--target <name>]
specgate capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]
specgate replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]
```

`discover` exports a deterministic CTSC registry from Rust link-time or C#
compiled-assembly metadata. `capture` runs ordinary Rust tests in isolation
and records passing native scenarios. `replay` verifies a capture bundle,
statically links its top-level semantic inputs to a Rust candidate, and
emits an independent deterministic CTSC trace. No command reads
`.spec.yaml`.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.