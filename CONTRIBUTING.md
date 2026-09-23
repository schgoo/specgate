# Contributing to SpecGate

## Setup

1. Install the pinned Rust toolchain from `rust/rust-toolchain.toml`.
2. Install `just`, `cargo-deny`, and `cargo-doc2readme`.
3. Install Python dependencies from `docs/ctsc/requirements.txt`.
4. Install the .NET 10 SDK.

## CTSC-first development

Behavior changes must update or add focused registry, native trace, capture, or
replay tests. Capture behavior at real operation boundaries through ordinary
tests; do not introduce a parallel assertion language or flat trace sink.

Use `binding-schema.json` for Rust/C# target bindings. The active CLI commands
are `discover`, `capture`, and `replay`.

## Gate

Run from the repository root:

```powershell
just check
```

The gate covers Rust build/tests/clippy/fmt/licenses, generated crate READMEs,
the CTSC Python corpus and linked validators, a deterministic capture-to-replay
smoke, and the retained C# build/tests/format/analyzers.

Run `just package-smoke` for release changes; it packages all six retained crates,
installs the packaged CLI, and exercises registry-dependency
discover/capture/replay.

Use Conventional Commits and keep changes scoped.
