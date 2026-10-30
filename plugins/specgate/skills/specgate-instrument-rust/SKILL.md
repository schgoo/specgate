---
name: specgate-instrument-rust
description: "Annotates an existing Rust crate for SpecGate capture with spec_component!, #[spec_operation], #[spec_setup], and #[derive(SpecEvent)], including async fn operations. Use when adding SpecGate annotations to Rust code, writing a Rust port that must match a reference, or fixing Rust annotation compile errors."
---

# Instrument Rust with SpecGate

Part of the SpecGate agent plugin 0.6.0. It covers using SpecGate in the user's project, not developing SpecGate itself.

## Path resolution

`${PLUGIN_DIR}` is the plugin root, the directory that holds `.claude-plugin/plugin.json`.

- **Claude Code:** `${PLUGIN_DIR}` is `${CLAUDE_PLUGIN_ROOT}`.
- **Other agents:** this file is `skills/specgate-instrument-rust/SKILL.md`, so the plugin root is two directories up.

`${PLUGIN_DIR}/docs/` holds every SpecGate guide and reference page. Links to `https://github.com/schgoo/specgate/…/v0.6.0/` show SpecGate's own sources and examples at this release. Read them as examples; do not adopt SpecGate's contributor workflow (`just` recipes, the golden matrix) in the user's project.

## Instrument a Rust crate

### Goal

Annotate a Rust crate so `specgate` can discover its component surface and
capture its tests as reference behavior.

### Prerequisites

- The CLI ([`install.md`](${PLUGIN_DIR}/docs/guides/install.md)).
- `specgate` added to the crate: `cargo add specgate`.
- Ordinary `#[test]` functions that call the operations you annotate. Capture
  records only what tests actually run. Annotate the requested API and reuse
  the tests that already call it; add tests only for uncovered behavior
  ([choose the boundary](${PLUGIN_DIR}/docs/guides/README.md#step-0-choose-the-boundary-before-instrumenting)).

### Steps

#### 1. Declare the component and its operations

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

#### 2. Annotate structured types

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
- See [`../reference/value-mapping.md`](${PLUGIN_DIR}/docs/reference/value-mapping.md) for
  supported types.

#### 3. Annotate setups for stateful operations

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
Cargo test target ([limitations](${PLUGIN_DIR}/docs/reference/limitations.md)).

For simple parameter objects, add `#[spec_event(setup)]` beside
`#[derive(SpecEvent)]`. Without member annotations, every field is selected;
otherwise only annotated fields are selected. Selected fields must be public
and become flat operation inputs. Replay decodes them into a struct literal,
using `..Default::default()` for omitted fields. The opt-in promises that those
defaults preserve the contract; use an explicit producer for construction
invariants. Supported field inputs include recursive `Vec`, `Option`, sets, and
string-keyed maps. A field whose type is itself `#[spec_event(setup)]` is
deconstructed recursively, including in setup-producer arguments; records inside
containers are not. Leaf names are flat and never prefixed, so rename a
colliding field with `#[spec_event(name = "...")]`. Duplicate names and types
that reach themselves are rejected at discovery and capture. Ordinary
`SpecEvent` projection does not enable reconstruction.

#### 4. Return outcomes

| Return type | Recorded outcome |
| --- | --- |
| `T` | `result` |
| `()` | No result |
| `Option<T>` | `result` when `Some`, `empty` when `None` |
| `Result<T, E>` | `result` when `Ok`, `error` when `Err` |
| Panic | An unexpected target fault |

#### 5. Optional observations

`spec_trace!("cache.hit", true);` records an observation inside the current
operation. Observations are not declared in the registry yet, so a bundle that
contains one fails linked validation
([limitations](${PLUGIN_DIR}/docs/reference/limitations.md)).

#### 6. Async operations

`async fn` operations are supported; see [`async-capture.md`](${PLUGIN_DIR}/docs/guides/async-capture.md).

#### 7. Explicit generic specializations

Declare each concrete specialization with its own semantic operation name:
`#[spec_operation("find_items", types = "crate::Item")]`. Repeat the annotation
for other specializations; use comma-separated types for multiple parameters.
Type paths resolve in the operation's module, and generic constraints still
apply. Discovery never infers specializations from test calls. See
[`../status.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/status.md#candidate-replay-and-explicit-generic-operations)
for unsupported specialization shapes.

#### 8. `const fn` operations

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

(the `spec_operation` macro doc tests compile the rejection and this
wrapper)

- Capture records only calls that go through the wrapper. Tests must call
  `checked_add`, not `add`.
- Const contexts (`const`, `static`, array lengths) keep calling `add`.

### Expected output

`cargo test` passes unchanged: annotations record nothing unless `specgate
capture` runs the tests. Then:

```sh
specgate capture --component example.math --out capture
```

prints `Complete(component=example.math, scenarios=…, …)` and exits 0.

### Artifacts

None from instrumentation. Capture writes a bundle; see
[`capture-a-reference.md`](${PLUGIN_DIR}/docs/guides/capture-a-reference.md).

### Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `native capture cannot instrument an async setup` | An `async fn` is a `#[spec_setup]`. | Make the setup synchronous; the component is otherwise discovery-only. |
| `capture needs runtime state that const evaluation` | A `const fn` has `#[spec_operation]` or `#[spec_setup]`. | Annotate a non-const wrapper and call it from tests ([step 8](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md#8-const-fn-operations)). |
| `ran twice in one capture with different inputs` | One test built the same setup twice with different inputs before calling the operation. | Construct once per test, or use identical inputs. |
| `cannot fold its #[spec_setup] declarations` | Several setups claim the same receiver or parameter. | Make each setup fill exactly one receiver or parameter. |
| `no passing tests captured operations for component` | No passing test called an operation of the component. | Add a test that calls it; check the listed skipped tests. |

More in [`troubleshoot-capture.md`](${PLUGIN_DIR}/docs/guides/troubleshoot-capture.md).

### Verify

```sh
specgate validate bundle capture
```

prints `valid: bundle capture` and exits 0.

## Async capture

### Goal

Capture async operations correctly in Rust and C#, and know which shapes are
not captured.

### Prerequisites

- An instrumented implementation
  ([Rust](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md) or [C#](${PLUGIN_DIR}/docs/guides/instrument-csharp-assembly.md)).
- Familiarity with [`capture-a-reference.md`](${PLUGIN_DIR}/docs/guides/capture-a-reference.md).

### Steps

#### Rust

1. Annotate `async fn` exactly like a synchronous function:

   ```rust
   use specgate::spec_operation;

   #[spec_operation("fetch", spec = "fixture.async_fetch")]
   pub async fn fetch(url: &str) -> String {
       std::future::ready(format!("response from {url}")).await
   }
   ```

2. Drive the top-level future to completion on the test thread, for example
   with `smol::block_on(fetch(…))`, as the repository's async fixtures do.
3. Keep setups synchronous.

#### C#

1. Annotate methods returning `Task`, `Task<T>`, `ValueTask`, or `ValueTask<T>`:

   ```csharp
   [SpecOperation("fetch", Spec = "fixture.cli_capture.fetch")]
   public static async Task<string> Fetch([SpecInput("url")] string url) =>
       await Task.FromResult($"response from {url}");
   ```

2. Await every annotated task before the test ends.
3. Keep setups synchronous.

### Expected output

The same `Complete(...)` line and bundle as synchronous capture.

### Artifacts

A normal bundle. Each async operation is one operation span with the awaited
value as its result.

### Behavior

| Behavior | Rust | C# |
| --- | --- | --- |
| Supported shapes | `async fn` operations | `Task`, `Task<T>`, `ValueTask`, `ValueTask<T>` |
| Span opens | At the future's first poll; a future never polled records nothing | When the method is called |
| Recorded result | The output value | The awaited value; `Task` and `ValueTask` record none |
| Thread migration | A future built inside a recording operation keeps its parent when polled on another thread | Supported |
| Failure | `Err` / panic as for sync code | A faulted task is recorded like a throw and rethrown |
| Abandonment | Dropped while pending: `conformance.abandoned`, status `UNSET`, not propagated to the parent | Cancellation (`IsCanceled`, or `OperationCanceledException` from a `ValueTask`): same marker and status; persisted immediately, so top-level cancellation keeps the scenario |
| Never finished | Top-level abandonment, or work still open at test end: the scenario is dropped with a diagnostic | An unawaited task still outstanding when the test's session finishes: the open span is recorded as a fault and `InvalidOperationException` is thrown, so that test contributes no scenario |
| Rejected | Async setups | Async setups, `async void`, async iterators, `async` methods returning other awaitables |
| Replay | Directly awaited free functions and setup-backed methods use the candidate's Arty, Smol, Tokio, or custom execution wrapper | Captured C# stimuli can replay against Rust async candidates; C# candidate replay is unsupported |

Wrap a future you intend to abandon inside an enclosing annotated operation
that completes; the abandonment is then recorded on the inner span
([`decisions/top-level-abandonment-unsupported.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/top-level-abandonment-unsupported.md)).

Not covered by tests: handing the top-level annotated future to a
multi-threaded executor, or reaching an operation through a raw
`std::thread::spawn`
([`decisions/async-capture-context.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/async-capture-context.md)).
Avoid both in capture tests.

Async replay does not reproduce overlapping sibling scheduling. It invokes
recorded top-level operations sequentially and observes nested calls made by the
candidate. Nested abandonment can replay through a completing enclosing
operation; top-level abandonment remains unsupported. Generic async operations
require explicit concrete specializations, and setup producers must remain
synchronous. See [`replay-a-candidate.md`](${PLUGIN_DIR}/docs/guides/replay-a-candidate.md).

### Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `was still open when the capture ended` | A Rust operation never completed, for example a top-level future dropped while pending. | Await it, or enclose it in an annotated operation that completes. |
| `native capture cannot instrument an async setup` | Rust async `#[spec_setup]`. | Make the setup synchronous. |
| `Async capture supports operations returning Task, Task<T>, ValueTask, or ValueTask<T>` | C# `async void`, async setup, or an `async` method returning another awaitable. | Return a supported type; make setups synchronous. |
| `unsupported async replay scheduling:` | Recorded sibling operations overlap. | Record directly awaited calls; overlapping scheduling is unsupported. |

### Verify

`specgate validate bundle <capture-dir>` exits 0, and the trace contains one
operation span per awaited call.
