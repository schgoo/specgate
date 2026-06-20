# Specgate

[![crates.io](https://img.shields.io/crates/v/specgate.svg)](https://crates.io/crates/specgate)
[![docs.rs](https://docs.rs/specgate/badge.svg)](https://docs.rs/specgate)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

## `SpecGate`

Deterministic spec-based verification for LLM-implemented code.

Engineers write specs. LLMs implement them. `SpecGate` closes the gap by
providing a non-stochastic harness that validates implementations against
specs using runtime traces.

### Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
specgate = "0.1"

[dev-dependencies]
specgate = { version = "0.1", features = ["harness"] }
```

Annotate your code:

```rust
use specgate::{spec_operation, SpecEvent};

#[derive(SpecEvent)]
struct Point { x: i32, y: i32 }

#[spec_operation("add_points")]
fn add_points(a: Point, b: Point) -> Point {
    Point { x: a.x + b.x, y: a.y + b.y }
}
```

Run your spec:

```rust
#[test]
fn spec_passes() {
    let result = specgate::run_spec("specs/my-component.spec.yaml");
    assert!(matches!(result, specgate::RunOutcome::Complete { .. }));
}
```

### CLI

Install the companion CLI for command-line validation and execution:

```bash
cargo install specgate-cli
specgate validate specs/
specgate run specs/my-component.spec.yaml
```

### Features

* **`harness`** — enables `run_spec()` and the test harness (add to `[dev-dependencies]`)
* **`trace`** — enables runtime trace collection (required for harness, zero-cost when off)


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

