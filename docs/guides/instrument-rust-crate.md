# Instrument a Rust crate

## Goal

Annotate a Rust crate so `specgate` can discover its component surface and
capture its tests as reference behavior.

## Prerequisites

- The CLI ([`install.md`](install.md)).
- `specgate` added to the crate: `cargo add specgate`.
- Ordinary `#[test]` functions that call the operations you annotate. Capture
  records only what tests actually run. Annotate the requested API and reuse
  the tests that already call it; add tests only for uncovered behavior
  ([choose the boundary](README.md#step-0-choose-the-boundary-before-instrumenting)).

## Steps

### 1. Declare the component and its operations

```rust ignore
use specgate::{spec_component, spec_operation};

spec_component!("example.math");

#[spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[test]
fn adds_two_numbers() {
    assert_eq!(add(2, 3), 5);
}
```

[doctest] (the `specgate` crate doc test compiles the same pattern)

- Declare `spec_component!("…")` once at the crate root; it sets the crate-wide
  default component. Override it per item with `spec = "…"`, for example
  `#[spec_operation("add", spec = "example.math")]`.
- Each library, binary, or integration-test target needing discovery/replay
  hooks must have its own crate-root declaration, even if every operation uses
  an explicit `spec`. A library's declaration does not cover separate Cargo
  targets. No generated test is added during ordinary `cargo test` runs.
  A generated-build error naming
  `__specgate_requires_crate_root_spec_component` means this declaration is
  missing. A hook-only default does not enter the registry unless an operation
  or type uses that component.
- The operation name is the first argument. Operations must be public; discovery
  rejects annotated private functions.
- Parameter names become input names. Rename one with
  `#[spec_input("name")]` on the parameter.
- The macros resolve your actual `specgate` dependency, so a renamed
  dependency (`sg = { package = "specgate", … }`) works.

### 2. Annotate structured types

Types that cross an operation boundary need `#[derive(SpecEvent)]`:

```rust
use specgate::{SpecEvent, spec_operation};

#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
#[spec_component("comp.core")]
pub struct Widget {
    #[spec_event]
    pub id: i32,
    #[spec_event]
    pub label: String,
}

#[spec_operation("make_widget", spec = "comp.core")]
pub fn make_widget() -> Widget {
    Widget { id: 1, label: "widget".to_string() }
}
```

- On a struct, only fields marked `#[spec_event]` are recorded. Rename one with
  `#[spec_event(name = "…")]`. Structs must use named fields.
- On an enum, each variant is recorded with all of its fields.
- See [`../reference/value-mapping.md`](../reference/value-mapping.md) for
  supported types.

### 3. Annotate setups for stateful operations

A method operation needs a setup that constructs its receiver:

```rust
use specgate::{SpecEvent, spec_operation, spec_setup};

#[derive(Debug, PartialEq, Eq, SpecEvent)]
#[spec_component("fixture.setup")]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("increment", spec = "fixture.setup")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.setup")]
    pub fn increment(&mut self) {
        self.count += 1;
    }
}
```

The setup's inputs (`initial`) become the operation's inputs. Use
`fills = "param"` when a setup produces a parameter instead of the receiver.
Replay reconstructs receivers and injected parameters through annotated
synchronous nongeneric producers, including private producers in a selected
Cargo test target ([limitations](../reference/limitations.md)).

<!-- docs-claim property-setup-depth: rust/crates/specgate-discovery/src/discovery/setup.rs::fold_operation rust/crates/specgate-cli/src/replay/link.rs::link_operation rust/crates/specgate-annotations-macros-impl/src/setup.rs::expand_setup csharp/SpecGate.Runtime/SetupStaging.cs -->
For simple parameter objects, add `#[spec_event(setup)]` beside
`#[derive(SpecEvent)]`. Without member annotations, every field is selected;
otherwise only annotated fields are selected. Selected fields must be public
and become flat operation inputs. Replay decodes them into a struct literal,
using `..Default::default()` for omitted fields. The opt-in promises that those
defaults preserve the contract; use an explicit producer for construction
invariants. Supported field inputs include recursive `Vec`, `Option`, sets, and
string-keyed maps. Ordinary `SpecEvent` projection does not enable reconstruction.
<!-- /docs-claim property-setup-depth -->

### 4. Return outcomes

| Return type | Recorded outcome |
| --- | --- |
| `T` | `result` |
| `()` | No result |
| `Option<T>` | `result` when `Some`, `empty` when `None` |
| `Result<T, E>` | `result` when `Ok`, `error` when `Err` |
| Panic | An unexpected target fault |

### 5. Optional observations

`spec_trace!("cache.hit", true);` records an observation inside the current
operation. Observations are not declared in the registry yet, so a bundle that
contains one fails linked validation
([limitations](../reference/limitations.md)). [doctest]

### 6. Async operations

`async fn` operations are supported; see [`async-capture.md`](async-capture.md).

### 7. Explicit generic specializations

Declare each concrete specialization with its own semantic operation name:
`#[spec_operation("find_items", types = "crate::Item")]`. Repeat the annotation
for other specializations; use comma-separated types for multiple parameters.
Type paths resolve in the operation's module, and generic constraints still
apply. Discovery never infers specializations from test calls. See
[`../status.md`](../status.md#candidate-replay-and-explicit-generic-operations)
for unsupported specialization shapes.

## Expected output

`cargo test` passes unchanged: annotations record nothing unless `specgate
capture` runs the tests. Then:

```sh
specgate capture --component example.math --out capture
```

prints `Complete(component=example.math, scenarios=…, …)` and exits 0. [smoke]

## Artifacts

None from instrumentation. Capture writes a bundle; see
[`capture-a-reference.md`](capture-a-reference.md).

## Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `native capture cannot instrument an async setup` | An `async fn` is a `#[spec_setup]`. | Make the setup synchronous; the component is otherwise discovery-only. |
| `ran twice in one capture with different inputs` | One test built the same setup twice with different inputs before calling the operation. | Construct once per test, or use identical inputs. |
| `cannot fold its #[spec_setup] declarations` | Several setups claim the same receiver or parameter. | Make each setup fill exactly one receiver or parameter. |
| `no passing tests captured operations for component` | No passing test called an operation of the component. | Add a test that calls it; check the listed skipped tests. |

More in [`troubleshoot-capture.md`](troubleshoot-capture.md).

## Verify

```sh
specgate validate bundle capture
```

prints `valid: bundle capture` and exits 0. [smoke]

## Source of truth

- `rust/crates/specgate/src/lib.rs` — facade and doc test.
- `rust/crates/specgate-annotations-macros-impl/src/` — macro arguments.
- Fixtures under [`../../test/behavior/cases/`](../../test/behavior/cases/),
  for example `fixture.setup/rust/setup.rs` and `comp.core/rust/component.rs`.
- [`decisions/rust-semantic-identities-and-capture-api.md`](../decisions/rust-semantic-identities-and-capture-api.md)
