# Specgate-Harness

[![crates.io](https://img.shields.io/crates/v/specgate-harness.svg)](https://crates.io/crates/specgate-harness)
[![docs.rs](https://docs.rs/specgate-harness/badge.svg)](https://docs.rs/specgate-harness)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` harness — entry point.

`run_spec(path)` loads a spec, locates the fixture source via the
binding, generates a temporary Cargo project that includes the
fixture and invokes its annotated functions, shells out to
`cargo run` to compile + execute, then reads emitted traces back
and subsequence-matches against each case’s `expected:` list.

The harness **never** parses or interprets the fixture source itself.
It only scans for attribute names and signatures (to validate the
spec references real symbols and to know how to call them), and
delegates everything else to the real Rust toolchain.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

