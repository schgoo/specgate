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
specgate validate registry <registry.json> [--import <registry.json>]...
specgate validate trace <trace.otlp.json|trace.otlp.jsonl>
specgate validate linked <trace> <root-registry> [--import <registry.json>]...
specgate validate bundle <capture-dir>
specgate compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...
```

`discover` exports a deterministic CTSC registry from Rust link-time or C#
compiled-assembly metadata. `capture` runs ordinary Rust tests in isolation
and records passing native scenarios. `replay` verifies a capture bundle,
statically links its top-level semantic inputs to a Rust candidate, and
emits an independent deterministic CTSC trace. `validate` provides native
CTSC 0.2 Registry, Trace Core, Linked, and capture-bundle validation.
`compare` applies deterministic `ctsc.strict/0.1.0` semantics. No command
reads `.spec.yaml` or invokes Python.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.