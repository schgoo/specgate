# Changelog

## [Unreleased — 0.6.0]

### Changed

- Registry documents now order their `components` array by ascending component
  id in every encoder. The selected component no longer leads its own
  dependencies, so `discover` and `capture` emit byte-identical bytes for the
  same component set. `registryId` is unchanged.
- Component capture no longer rejects a scenario that calls into other
  annotated components. It now exports the selected component's top-level
  operation subtrees verbatim, keeping nested foreign operations with their
  original parentage, and emits a registry that is the union of every component
  present in the filtered trace.
- Replaced the legacy verification stack with CTSC-native discovery, capture,
  and replay.
- Added the focused `specgate-discovery` crate and one strict target-binding
  resolver shared by all CLI commands.
- Reduced Rust and C# fixtures to stateless, rich-type, and setup-discovery
  coverage.

### Removed

- Spec-driven validation, execution, matching, extraction, code generation,
  coverage, and self-hosting.
- Flat trace recording and table-driven mock instrumentation.
- The former harness/types crates and C# runtime/weaver projects.
- The standalone Rust `specgate-annotations` facade; `specgate` is the sole
  macro/runtime facade.
