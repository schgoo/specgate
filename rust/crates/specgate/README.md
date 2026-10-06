# Specgate

[![crates.io](https://img.shields.io/crates/v/specgate.svg)](https://crates.io/crates/specgate)
[![docs.rs](https://docs.rs/specgate/badge.svg)](https://docs.rs/specgate)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../../LICENSE-MIT)

Umbrella crate for `SpecGate`’s native CTSC annotation surface.

Add `specgate` to an implementation crate, declare a component, annotate
operations/setups/types, and exercise behavior through ordinary tests.
`specgate capture` records those real invocations as deterministic CTSC
reference traces; `specgate replay` invokes a candidate from the captured
semantic inputs. An async operation records from its first poll under the
operation that constructed its future, so it survives migration to another
executor thread; an async setup is not yet instrumented.

```rust
use specgate::*;

spec_component!("example.math");

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    add_impl(a, b)
}

fn add_impl(a: i32, b: i32) -> i32 {
    a + b
}

fn main() {}
```


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.