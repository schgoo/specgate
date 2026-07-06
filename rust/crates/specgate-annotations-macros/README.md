# Specgate-Annotations-Macros

[![crates.io](https://img.shields.io/crates/v/specgate-annotations-macros.svg)](https://crates.io/crates/specgate-annotations-macros)
[![docs.rs](https://docs.rs/specgate-annotations-macros/badge.svg)](https://docs.rs/specgate-annotations-macros)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

Procedural macros for `SpecGate` annotations.

* `#[spec_operation("name")]` — marks a function as a spec operation; emits a
  `$run` event, per-parameter input events, and a `$result`/`$outcome`.
* `#[spec_setup("name")]` — marks a constructor/setup that builds the
  receiver for stateful (method) operations.
* `#[spec_mock(...)]` — injects a table-driven mock dependency.
* `#[derive(SpecEvent)]` + `#[spec_event]` — capture struct/enum fields into
  the trace as structured values.
* `#[spec_input("name")]` — give a parameter a language-neutral spec name.
* `spec_component!("name")` — declare the crate’s component (the spec name).
* `spec_trace!(...)` — emit an inline trace checkpoint from within a body.

These expand into calls into `::specgate_annotations::__rt` (which
re-exports `specgate-runtime`); the expanded code emits real trace events at
runtime.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

