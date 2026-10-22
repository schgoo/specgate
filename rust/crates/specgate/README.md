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
semantic inputs.

Async operations open at first poll and keep their construction parent
across executor-thread migration and same-thread interleaving. A future
dropped while pending is recorded as abandoned. Async setups are
unsupported. `ComponentId`, `OperationName`, and `ImplementationLabel`
distinguish semantic identities while retaining transparent CTSC string
projection and unrestricted string input.

Task guides, including
[instrumenting a Rust crate][__link0],
are in the [SpecGate repository][__link1].

```rust
use specgate::{spec_component, spec_operation};

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

 [__link0]: https://github.com/schgoo/specgate/blob/main/docs/guides/instrument-rust-crate.md
 [__link1]: https://github.com/schgoo/specgate/tree/main/docs/guides
