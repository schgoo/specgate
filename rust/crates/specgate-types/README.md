# Specgate-Types

[![crates.io](https://img.shields.io/crates/v/specgate-types.svg)](https://crates.io/crates/specgate-types)
[![docs.rs](https://docs.rs/specgate-types/badge.svg)](https://docs.rs/specgate-types)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` types — the typed data model for specs, bindings, and run
results, shared by the CLI and the harness so structure is defined once.

* `spec_document` — the parsed `.spec.yaml` representation (`SpecDocument`,
  `SpecCase`, `TestStep`, …) plus `validate_spec_document` for
  schema-level validation.
* `binding_file` — the parsed binding YAML (`BindingFile`, `BindingTarget`).
* `report` — run-result types (`RunOutcome`, `RunReport`, `CaseResult`,
  `CaseStatus`, `RunError`).

These are plain serde-parsed data types with no behavior of their own.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

