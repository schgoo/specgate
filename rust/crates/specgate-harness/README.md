# Specgate-Harness

[![crates.io](https://img.shields.io/crates/v/specgate-harness.svg)](https://crates.io/crates/specgate-harness)
[![docs.rs](https://docs.rs/specgate-harness/badge.svg)](https://docs.rs/specgate-harness)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` harness — compiles annotated code, runs its operations, collects
runtime traces, and matches them against a spec’s expected assertions.

`run_spec(path)` loads a spec, resolves its binding, and for each case
generates a temporary Cargo project (the “runner”) that LINKS the target
crate as a dependency and calls its public operations, shells out to
`cargo run` to compile + execute, then reads the emitted traces back and
subsequence-matches them against each case’s `expected:` list.

Key design points:

* A spec is a contract over a component’s PUBLIC API, so the harness reaches
  each operation only through the target crate’s public path
  (`use <crate>[::<module>] as fut;`) — it never inlines or interprets the
  source. A non-public annotated operation is rejected up front with a
  “not publicly reachable” diagnostic.
* It scans source only for attribute names and signatures (to validate the
  spec references real symbols and to know how to call them); everything
  else is delegated to the real Rust toolchain.
* Matching is a subsequence match with a rich operator set; async operations
  are driven on a per-target runtime (`smol` or `tokio`).


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

