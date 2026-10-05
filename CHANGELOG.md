# Changelog

Entries are one line per change. Rationale lives in the commit body, an ADR
under `docs/decisions/`, or `docs/roadmap.md`. Generate with `just changelog`.

## [Unreleased]

### Added

- `#[spec_operation]` instruments a directly-awaited `async fn`, recording from
  first poll through completion.
- `finish_native_capture` closes outstanding operations with an
  `incomplete_capture` target fault and persists the sidecar.
- The capture sidecar is a continuously valid provisional snapshot, rewritten at
  each operation's recorded inputs and at every close.
- `specgate-discovery`, a focused crate owning one strict target-binding
  resolver shared by all CLI commands.

### Changed

- Native capture state is owned by a per-run `Send` collector rather than a
  thread-local, which is the precondition for carrying context into a future.
- Trace validation rejects a `conformance.operation` span carrying no completion
  or failure event unless its status is `OK`.
- Registry documents order `components` by ascending id in every encoder, so
  `discover` and `capture` emit byte-identical bytes.
- Component capture exports top-level operation subtrees verbatim instead of
  rejecting scenarios that call into other annotated components.
- The CTSC-native discovery, capture, and replay stack replaces the legacy
  verification stack.
- Rust and C# fixtures are reduced to stateless, rich-type, and setup-discovery
  coverage.

### Testing

- `fixture.async_fetch`, `fixture.extract`, `fixture.async_smol_timer`, and
  `fixture.async_tokio_timer` graduate from discovery-only rows to linked
  capture rows. No row carries `async-capture-unsupported`.
- `fixture.fallible_unit` drives its async operation but stays discovery-only on
  a new `observation-not-declared` limitation, pending M5.
- A regression test pins that two operations polled concurrently on one thread
  fail closed rather than recording a wrong trace.

### Removed

- Spec-driven validation, execution, matching, extraction, code generation,
  coverage, and self-hosting.
- Flat trace recording and table-driven mock instrumentation.
- The former harness/types crates and C# runtime/weaver projects.
- The standalone `specgate-annotations` facade; `specgate` is the sole facade.
