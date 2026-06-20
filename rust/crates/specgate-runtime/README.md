# Specgate-Runtime

[![crates.io](https://img.shields.io/crates/v/specgate-runtime.svg)](https://crates.io/crates/specgate-runtime)
[![docs.rs](https://docs.rs/specgate-runtime/badge.svg)](https://docs.rs/specgate-runtime)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` runtime — thread-local trace buffer + mock table + `SpecEvent` /
`ToSpecValue` traits + structured `Value` type.

Companion to the `specgate-annotations` proc-macro crate. The macros
expand into calls into this runtime; user code never references this
crate directly.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

