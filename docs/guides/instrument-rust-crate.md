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

```rust
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
  default component. Override it per item with `component = "…"`, for example
  `#[spec_operation("add", component = "example.math")]`.
- Both `#[spec_operation]` and `#[spec_setup]` use `component = "…"` for an
  explicit component. The old `spec = "…"` spelling is rejected, not retained as
  an alias. Replace it with `component = "…"`; component identities and captured
  CTSC values do not change. Duplicate `component` arguments are rejected, even
  when their values match
  ([compatibility decision](../decisions/rust-component-selector.md)).
- Each library, binary, or integration-test target needing discovery/replay
  hooks must have its own crate-root declaration, even if every operation uses
  an explicit `component`. A library's declaration does not cover separate Cargo
  targets. No generated test is added during ordinary `cargo test` runs.
  A generated-build error naming
  `__specgate_requires_crate_root_spec_component` means this declaration is
  missing. A hook-only default does not enter the registry unless an operation
  or type uses that component.
- The operation name is the first argument. Operations must be public; discovery
  rejects annotated private functions.
- Parameter names become input names. Rename one with
  `#[spec_input("name")]` on the parameter.
- Omit nonsemantic operation or synchronous setup context with
  `#[spec_input(ignore)]` or `#[spec_input("context", ignore)]`. The type must
  implement `Default`, but needs no `ToNativeValue`. Ordinary calls retain the
  supplied context; only replay constructs its default in the native argument
  position. For record or enum payload context, use `#[spec_event(ignore)]`;
  C# reference record fields/properties use `[SpecEvent(Ignore = true)]`.
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

#[spec_operation("make_widget", component = "comp.core")]
pub fn make_widget() -> Widget {
    Widget { id: 1, label: "widget".to_string() }
}
```

- On a struct, only fields marked `#[spec_event]` are recorded. Rename one with
  `#[spec_event(name = "…")]`. Structs must use named fields.
- On an enum, each variant is recorded with all non-ignored fields.
- `#[spec_event(ignore)]` excludes a field before projection or type mapping.
  Ignored tuple positions are compacted; an all-ignored tuple stays an empty
  tuple payload, not a unit variant. Ignored types need no projection bounds.
- See [`../reference/value-mapping.md`](../reference/value-mapping.md) for
  supported types.

Native tuples of arity 2-4 are supported recursively. Independent discovery
still declares `Vec<T>` as a list, but replay can bind it to reference bags,
sets, or ordered sets. Pair vectors can store ordered/non-string maps.
See [replay storage](../reference/value-mapping.md#candidate-replay-values).

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

#[spec_setup("increment", component = "fixture.setup")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("increment", component = "fixture.setup")]
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

For simple parameter objects, add `#[spec_event(setup)]` beside
`#[derive(SpecEvent)]`. Without member annotations, every field is selected;
otherwise only annotated, non-ignored fields are selected. Ignored-only
annotations select zero fields, not all-minus-ignored. Selected fields must be public
and become flat operation inputs. Replay decodes them into a struct literal,
using the whole struct's `Default` for omitted fields when available. If only
explicitly ignored fields are omitted and the struct has no `Default`, their
field-type defaults are the fallback. Other unannotated omissions still require
the whole-struct default. An explicit producer owns its output and replay never
resets ignored fields afterward. Selected inputs decode before any default.
The opt-in promises that those
defaults preserve the contract; use an explicit producer for construction
invariants. Supported field inputs include recursive `Vec`, `Option`, sets, and
string-keyed maps. A field whose type is itself `#[spec_event(setup)]` is
deconstructed recursively, including in setup-producer arguments; records inside
containers are not. Leaf names are flat and never prefixed, so rename a
colliding field with `#[spec_event(name = "...")]`. Duplicate names and types
that reach themselves are rejected at discovery and capture. Ordinary
`SpecEvent` projection does not enable reconstruction.

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

### 8. `const fn` operations

`#[spec_operation]` and `#[spec_setup]` reject `const fn`. Capture needs
runtime state, and const evaluation cannot use it. Keep the `const fn` and
annotate a non-const wrapper that calls it:

```rust
pub const fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[spec_operation("add")]
pub fn checked_add(a: i32, b: i32) -> i32 {
    add(a, b)
}

#[test]
fn adds_two_numbers() {
    assert_eq!(checked_add(2, 3), 5);
}
```

[doctest] (the `spec_operation` macro doc tests compile the rejection and this
wrapper)

- Capture records only calls that go through the wrapper. Tests must call
  `checked_add`, not `add`.
- Const contexts (`const`, `static`, array lengths) keep calling `add`.

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
| `capture needs runtime state that const evaluation` | A `const fn` has `#[spec_operation]` or `#[spec_setup]`. | Annotate a non-const wrapper and call it from tests ([step 8](#8-const-fn-operations)). |
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
