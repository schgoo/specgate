# Specgate-Runtime

[![crates.io](https://img.shields.io/crates/v/specgate-runtime.svg)](https://crates.io/crates/specgate-runtime)
[![docs.rs](https://docs.rs/specgate-runtime/badge.svg)](https://docs.rs/specgate-runtime)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

`SpecGate` runtime — the support library the annotation macros expand into.

Provides the thread-local trace buffer, the mock table, the `SpecEvent` /
`ToSpecValue` traits, the structured `Value` type (the universal trace
value), and the link-time operation/type registry that
`specgate extract` reads to derive a spec from annotated code.

Companion to the `specgate-annotations` proc-macro crate: the macros expand
into calls into this runtime, so user code never references it directly.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

