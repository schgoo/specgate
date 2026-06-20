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

### Property Tests

Specs can declare property-based test cases that generate random inputs
and verify invariants across many iterations:

```yaml
cases:
  - name: add_commutative
    kind: property
    runs: 100
    generators:
      a: i32[-1000, 1000]
      b: i32[-1000, 1000]
    calls:
      forward: { operation: add, inputs: { a: "{a}", b: "{b}" } }
      reversed: { operation: add, inputs: { a: "{b}", b: "{a}" } }
    expected:
      - $assert: "forward.$result == reversed.$result"
```

Generator types: `i32[min, max]`, `f64[min, max]`, `bool`,
`string[min_len, max_len]`, `string[min, max, pattern: "regex"]`,
`oneof["a", "b"]`, `list[type, len: min..max]`,
`set[type, size: min..max]`, `map[key, value, size: min..max]`, `optional[type]`.

On failure, the `CaseResult` includes a `counterexample` with the shrunk
generator values that triggered the assertion failure, plus traces from
the failing run.

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

