# Specgate-Runtime

[![crates.io](https://img.shields.io/crates/v/specgate-runtime.svg)](https://crates.io/crates/specgate-runtime)
[![docs.rs](https://docs.rs/specgate-runtime/badge.svg)](https://docs.rs/specgate-runtime)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

`SpecGate` runtime — the support library the annotation macros expand into.

Provides native structured operation capture, semantic value projection,
and the link-time operation/type registry used by CTSC discovery.
Isolated test processes can activate native capture through
`SPECGATE_NATIVE_CAPTURE`; the first operation starts the session lazily and
each completed top-level operation atomically refreshes a stable JSON
sidecar containing the full scenario.

Companion to the `specgate-annotations-macros` proc-macro crate: the macros expand
into calls into this runtime, so user code never references it directly.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.