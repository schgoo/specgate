# SpecGate human digest

Concise status for reviewers and maintainers. Update this file when product
status, major limitations, ownership boundaries, or material risks change.

## Status

The legacy specification and matcher stack has been removed. SpecGate now uses
CTSC-native discovery, Rust reference capture, Rust candidate replay, native
validation, and strict comparison.

Rust and C# discovery produce language-neutral registries. Capture and replay
remain Rust-only.

## Current delivery boundary

The repository has no separate active roadmap. Work should start from the
current human directive, the CTSC contracts, and
`docs/specgate-ctsc-migration.md`, then be divided into reviewable vertical
slices.

## Major limitations

- Capture supports Rust libtest targets and rejects async operations before
  polling.
- Replay supports synchronous public Rust free functions with primitive
  lossless inputs.
- Setup-backed methods, structured replay values, explicit mappings, async
  replay, and C# replay are not implemented.
- Observation metadata is not discoverable, so observation-emitting components
  cannot yet produce linked bundles.
- Only the fixed `ctsc.strict/0.1.0` comparison policy is implemented.

## Risks and ownership

- CTSC document semantics, comparison behavior, and operation identity are
  compatibility boundaries and require an explicit human owner.
- Runtime dependency identity is load-bearing: substituting another
  `specgate-runtime` can split link-time metadata and capture state.
- The golden matrix is a trust anchor. Weakening coverage, parity, linkage, or
  negative-fixture checks can hide product regressions.

Human review remains the acceptance gate.
