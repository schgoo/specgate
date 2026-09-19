# Changelog

## [Unreleased — 0.6.0]

### Changed

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
