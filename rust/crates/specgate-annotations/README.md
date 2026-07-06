# Specgate-Annotations

[![crates.io](https://img.shields.io/crates/v/specgate-annotations.svg)](https://crates.io/crates/specgate-annotations)
[![docs.rs](https://docs.rs/specgate-annotations/badge.svg)](https://docs.rs/specgate-annotations)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` annotations — the public façade that annotated code depends on.

Re-exports the proc-macros from `specgate-annotations-macros`
(`#[spec_operation]`, `#[spec_setup]`, `#[spec_mock]`,
`#[derive(SpecEvent)]`, `#[spec_event]`, `#[spec_input]`,
`spec_component!`, `spec_trace!`) and the runtime support from
`specgate-runtime`. Annotated code typically does
`use specgate_annotations::*;` (or `use specgate::*;` via the umbrella
crate) to pull in everything at once.

Annotations are zero-cost in production: without the trace feature the
macros expand to no-ops.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

