# SpecGate human digest

Concise status for reviewers and maintainers. Update this file when product
status, major limitations, ownership boundaries, or material risks change.

## Status

The legacy specification and matcher stack has been removed. SpecGate now uses
CTSC-native discovery, Rust and C# reference capture, Rust candidate replay, native
validation, and strict comparison.

Rust and C# discovery produce language-neutral registries. Each captured
component has one shared native reference bundle directly in `component/<id>/`
(no `reference` subdirectory), explicitly selecting C# for
paired fixtures and Rust for Rust-only fixtures. Both producers still run strict
native capture and exact validation; producer registries and nonselected captures
are transient. There are no per-language successful-component goldens. The
selected full native registry byte-matches the single checked-in canonical;
other producers match it exactly or through the three exact known parity
exceptions. Those exceptions, unsupported replay, and link-only checks do not
claim behavioral equivalence. Discovery-only components keep a shared registry
oracle without behavioral files. Language is an endpoint
property, not its identity. Reference-to-candidate checks replay recorded stimuli
against Rust implementations, without pairing native test names across languages.
Candidate replay remains Rust-only.

## Current delivery boundary

The repository has no separate active roadmap. Work should start from the
current human directive, the CTSC contracts, and
`docs/specgate-ctsc-migration.md`, then be divided into reviewable vertical
slices.

## Major limitations

- Capture supports Rust libtest targets and C# native test projects. Async
  operation capture is supported; async setups are not instrumented.
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
