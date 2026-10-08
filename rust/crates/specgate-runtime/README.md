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
sidecar containing the full scenario. Operation and setup finalizers consume
their guards and report failure explicitly. Guard destruction performs only
infallible in-memory cleanup; it never persists, panics, or writes stderr.
Manually started sessions remain active until `finish`.
An annotated async operation captures the collector and parent at future construction, opens its span on first poll, and reinstalls context on every poll so migration and same-thread interleaving retain correct parentage. Setup staging remains thread-local, so async setups are unsupported.

Captured inputs are the registry’s black-box surface, not the raw call:
`#[spec_setup]` producers record their construction inputs, and the
operation they build adopts those inputs in place of the parameters the
setup fills. Attribution is by setup declaration, so running one declaration
twice in a capture is accepted only when both runs record value-identical
inputs; differing repeats are rejected rather than misattributed.

Basic operation entry uses owned semantic identities:

```rust
use specgate_runtime::{ComponentId, OperationName, capture};

let mut scope = capture::begin_operation(ComponentId::from("example"), OperationName::from("run"))?;
scope.unit()?;
```

Companion to the `specgate-annotations-macros` proc-macro crate: the macros expand
into calls into this runtime, so user code never references it directly.
`ComponentId`, `OperationName`, and `TargetName` provide semantic
string identities without imposing lexical validation.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.