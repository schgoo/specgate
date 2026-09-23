# Copilot Instructions — SpecGate

## Gate

**Always run `just check` from the repository root before committing.**

Useful focused recipes:

```powershell
just build
just test
just clippy
just format-check
just deny
just readme-check
just ctsc-validate
just ctsc-smoke
just package-smoke
just dotnet-build
just dotnet-test
just format-check-cs
```

## CTSC-first workflow

Behavior changes update or add focused CTSC registry, native trace, capture, or
replay tests. Capture behavior at real annotated operation boundaries through
ordinary tests. Do not add a separate behavioral assertion format or flat trace
compatibility path.

`binding-schema.json` is the CTSC target-binding format. Paths are relative to
the binding file. The CLI currently exposes only:

```text
specgate discover <binding> ...
specgate capture <binding> ...
specgate replay <capture> <candidate-binding> ...
```

## Architecture

1. `specgate-runtime` owns native capture, semantic values, and link-time
   operation/type metadata.
2. `specgate-annotations-macros` instruments operation boundaries and emits
   setup/type metadata; `specgate` is the sole Rust facade.
3. `specgate-discovery` owns strict binding resolution, Rust discovery builds,
   C# compiled-assembly reflection, raw invocation metadata, and deterministic
   semantic normalization/setup folding.
4. `specgate-ctsc` owns registry/trace encoding, bundle digest/link
   verification, replay models, and validators.
5. `specgate-cli` exposes discovery, Rust reference capture, and Rust candidate
   replay.

Async operations remain discoverable, but native capture rejects them before
polling because capture state is thread-local and cannot safely cross `.await`.

## Conventions

- Every annotated Rust crate declares `spec_component!("dotted.name")`.
- Public operations use `#[spec_operation("snake_case_name")]`.
- Setup selection is keyed by exact component + operation and must reject
  ambiguity.
- Structured values implement `ToNativeValue`, normally via
  `#[derive(SpecEvent)]`.
- C# annotations are metadata only; preserve raw declaring type, method,
  signature, static/public/async/setup, and exception information.
- Observation declarations and comparison profiles are not implemented; do not
  claim otherwise.
- Crate READMEs are generated from crate-level docs. Edit `lib.rs`, then run
  `just readme`.
- Use `ohno` for new Rust error types.
- Use LF except `.ps1`, `.cmd`, and `.bat`.
