# SpecGate agent plugin

Use SpecGate to check that two implementations of a component behave the same: install the CLI, annotate Rust or C# APIs, capture reference behavior from existing tests, replay it against a Rust port, and compare the traces.

Generated from the [SpecGate](https://github.com/schgoo/specgate) v0.6.0 user guides by `scripts/agent-plugin.ps1`. Do not edit these files; change the guides or `docs/contributor/agent-plugin.json` in SpecGate and run `just agent-plugin`.

## Skills

| Skill | Use |
| --- | --- |
| [`specgate`](skills/specgate/SKILL.md) | Plans and runs a SpecGate conformance check on the user's codebase: choose the API boundary, reuse existing tests, then discover, capture, validate, replay, and compare. Use when the user asks to add SpecGate, check that a Rust port matches a C# or Rust original, prove two implementations are equivalent, or asks which SpecGate step comes next. |
| [`specgate-install`](skills/specgate-install/SKILL.md) | Installs the specgate CLI, the C# discovery and weaver tools, the Rust specgate crate, and the SpecGate.Annotations NuGet package. Use when specgate is not on PATH, when cargo install specgate-cli or dotnet add package SpecGate.Annotations is needed, or when SPECGATE_DISCOVERY_PATH or SPECGATE_WEAVER_PATH errors appear. |
| [`specgate-instrument-rust`](skills/specgate-instrument-rust/SKILL.md) | Annotates an existing Rust crate for SpecGate capture with spec_component!, #[spec_operation], #[spec_setup], and #[derive(SpecEvent)], including async fn operations. Use when adding SpecGate annotations to Rust code, writing a Rust port that must match a reference, or fixing Rust annotation compile errors. |
| [`specgate-instrument-csharp`](skills/specgate-instrument-csharp/SKILL.md) | Annotates an existing C# assembly for SpecGate capture with [SpecOperation], [SpecSetup], [SpecInput], [SpecEvent], and [SpecException], including Task and ValueTask operations. Use when adding SpecGate attributes to C# code, choosing which C# tests become scenarios, or fixing C# discovery or weave errors. |
| [`specgate-capture`](skills/specgate-capture/SKILL.md) | Writes a specgate.yaml binding and runs specgate discover and specgate capture to record reference behavior from the user's existing Rust or C# tests into a CTSC bundle. Use when declaring components in specgate.yaml, recording a reference, or reading capture output. |
| [`specgate-replay`](skills/specgate-replay/SKILL.md) | Replays a captured SpecGate bundle against a Rust candidate implementation and walks through porting a component from C# or Rust to Rust. Use when running specgate replay, checking that a Rust port matches the original, or when replay reports an unsupported operation or input. |
| [`specgate-validate-compare`](skills/specgate-validate-compare/SKILL.md) | Validates SpecGate registries, traces, and bundles, and compares a candidate trace with the reference trace under ctsc.strict. Use when running specgate validate or specgate compare, or when explaining why compare reports equivalent: false. |
| [`specgate-troubleshoot`](skills/specgate-troubleshoot/SKILL.md) | Diagnoses failing specgate discover, capture, replay, validate, and compare commands from their error messages and exit codes. Use when a specgate command exits non-zero, reports a capture diagnostic or fault, or a C# weave or build step fails. |

## Contents

- `skills/`: one skill per task, each holding its guides.
- `docs/`: the guides and reference pages the skills link to.
- `binding-schema.json`: the JSON Schema for `specgate.yaml`.
