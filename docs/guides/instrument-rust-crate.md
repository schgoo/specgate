# Instrument a Rust crate

## Goal

Annotate a Rust crate so `specgate` can discover its component surface and
capture its tests as reference behavior.

## Prerequisites

- The CLI ([`install.md`](install.md)).
- `specgate` added to the crate: `cargo add specgate`.
- Ordinary `#[test]` functions that call the operations you annotate. Capture
  records only what tests actually run.

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
  default component. Override it per item with `spec = "…"`, for example
  `#[spec_operation("add", spec = "example.math")]`.
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

Discovery and replay build their internal test entry points in invocation
scratch, not beside your sources. Read-only source directories work with a
prepared Cargo lockfile and writable build/cache directories. Existing
`RUSTC_WORKSPACE_WRAPPER` and `build.rustc-workspace-wrapper` settings are
respected by chaining the selected wrapper; ordinary `RUSTC_WRAPPER` settings
remain in effect. A configured wrapper failure is reported as a build failure.

These are compiler wrappers, not a replacement Cargo workspace. Inherited
wrapper settings also apply to SpecGate's internal helper, discovery, and replay
scratch crates, so wrappers must accept those crates as well as your targets.
The contributor golden harness preserves these settings for intentional
build-negative fixtures too; wrapper errors cannot stand in for the expected
source diagnostics. Wrapper executable paths must be valid Unicode.

Each instrumented test-target build first runs a dependency-only compiler pass
against the original sources, then compiles the mirrored sources with the
crate-root hook. Proc macros can therefore execute twice, though the first pass
does not perform code generation or linking. Configured workspace wrappers run
on both passes and must support `--emit=dep-info` and rustc response files
(`@remappings`). Compatibility with a particular caching wrapper, such as
sccache, is not guaranteed; an incompatible wrapper must be reconfigured or
disabled explicitly rather than silently bypassed.

The mirror retains lexical source filenames, including `..`, and recreates
source symlinks with targets inside scratch so relative module and include
resolution stays intact. This requires permission to create symlinks in the
scratch filesystem; on Windows, enable Developer Mode or use an account allowed
to create symlinks. Paths whose `..` traversal would clamp at the filesystem
root are rejected rather than allowed to escape the mirror.

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
