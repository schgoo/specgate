---
name: specgate-instrument-csharp
description: "Annotates an existing C# assembly for SpecGate capture with [SpecOperation], [SpecSetup], [SpecInput], [SpecEvent], and [SpecException], including Task and ValueTask operations. Use when adding SpecGate attributes to C# code, choosing which C# tests become scenarios, or fixing C# discovery or weave errors."
---

# Instrument C# with SpecGate

Part of the SpecGate agent plugin 0.6.0. It covers using SpecGate in the user's project, not developing SpecGate itself.

## Path resolution

`${PLUGIN_DIR}` is the plugin root, the directory that holds `.claude-plugin/plugin.json`.

- **Claude Code:** `${PLUGIN_DIR}` is `${CLAUDE_PLUGIN_ROOT}`.
- **Other agents:** this file is `skills/specgate-instrument-csharp/SKILL.md`, so the plugin root is two directories up.

`${PLUGIN_DIR}/docs/` holds every SpecGate guide and reference page. Links to `https://github.com/schgoo/specgate/…/v0.6.0/` show SpecGate's own sources and examples at this release. Read them as examples; do not adopt SpecGate's contributor workflow (`just` recipes, the golden matrix) in the user's project.

## Instrument a C# assembly

### Goal

Annotate a C# project so `specgate` can discover its component surface and
capture its tests as reference behavior.

### Prerequisites

- The CLI with its C# tools beside it ([`install.md`](${PLUGIN_DIR}/docs/guides/install.md)).
- The .NET 10 runtime for the C# discovery tool (`SpecGate.Discovery` targets
  `net10.0`; building it from source needs the .NET 10 SDK).
- An SDK that builds your projects. The project that holds your operations must
  target .NET 8 or later for capture (`SpecGate.Runtime` targets `net8.0` and
  `net10.0`).
- A sources project that references the `SpecGate.Annotations` package.
- A separate tests project that references the sources project and uses exactly
  one of xUnit (`xunit` or `xunit.v3`), MSTest, or NUnit.

You do not edit either project for capture. `specgate capture` injects the IL
weaver and the `SpecGate.Runtime` reference into its own build of the sources
project.

### Steps

#### 1. Annotate operations

```csharp
using SpecGate.Annotations;

namespace Example;

public static class Arithmetic
{
    [SpecOperation("add", Spec = "fixture.cli_capture.add")]
    public static int Add([SpecInput("a")] int a, [SpecInput("b")] int b) => a + b;
}
```

- `Spec` is the component id and is required for capture.
- Repeat `[SpecOperation]` only for distinct explicit generic specializations.
- `[SpecInput("name")]` sets an input name; without it the parameter name is
  used.

#### 2. Reuse the existing tests

Capture runs the tests that already call your annotated API, unchanged. Start
there. Add a test only for behavior those tests do not cover, and report which
tests you added
([choose the boundary](${PLUGIN_DIR}/docs/guides/README.md#step-0-choose-the-boundary-before-instrumenting)).
A test must call the requested API itself, not a new wrapper that selects
hard-coded data by name.

```csharp
using Xunit;

public sealed class CaptureTests
{
    [Fact]
    public void AddsTwoNumbers()
    {
        Assert.Equal(5, Arithmetic.Add(2, 3));
    }
}
```

Each listed test becomes one scenario named by its display name. Theories and
parameterized tests produce one scenario per case.

#### 3. Annotate structured types

Mark each recorded type and member with `[SpecEvent]`. An optional argument
renames it.

```csharp
[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }
}
```

| C# declaration | CTSC type |
| --- | --- |
| Class or struct with `[SpecEvent]` | Record of its public `[SpecEvent]` properties and fields declared on that type |
| Abstract class with `[SpecEvent]` | Tagged union whose variants are its concrete `[SpecEvent]` subclasses |
| Enum with `[SpecEvent]` | Tagged union of its members, in declaration order, without payloads; `[SpecEvent("…")]` on a member renames it |

See [`../reference/value-mapping.md`](${PLUGIN_DIR}/docs/reference/value-mapping.md) for every
supported type and the values capture rejects.

#### 4. Declare outcomes

| Return or annotation | Recorded outcome |
| --- | --- |
| `T` | `result` |
| `void`, `Task`, `ValueTask` | No result |
| `SpecGate.Annotations.Option<T>` | `result` when present, `empty` when absent |
| `SpecGate.Annotations.Result<T, E>` | `result` when `Ok`, `error` when `Err` |
| `[SpecException(typeof(X))]` and a matching exception is thrown | `error` with the exception message as its value |
| `[SpecException]` with no types | Every exception is the `error` outcome |
| Any other exception | An unexpected target fault |

#### 5. Annotate setups for stateful operations

```csharp
[SpecEvent("Counter")]
public sealed class Counter
{
    [SpecEvent("count")]
    public int Count { get; set; }

    [SpecSetup("increment", Spec = "fixture.setup")]
    public static Counter Make([SpecInput("initial")] int initial) => new() { Count = initial };

    [SpecOperation("increment", Spec = "fixture.setup")]
    public void Increment() => Count++;
}
```

The setup's inputs become the operation's inputs. Set `Fills = "param"` when a
setup produces a parameter rather than the receiver. C# setup inputs can drive
annotated Rust candidate producers during replay; C# candidate replay remains
unsupported ([limitations](${PLUGIN_DIR}/docs/reference/limitations.md)).

`[SpecSetup]` may also annotate a constructor: its declaring type is the
produced value, and its parameters are the construction inputs.

For object-initializer parameters, use `[SpecEvent(Setup = true)]` on the type.
Without member annotations, all public instance fields and readable properties
declared on the type participate; otherwise only annotated members participate.
Capture reads their final values at operation entry and folds their semantic
names into flat inputs. A member whose type is itself `Setup = true` is folded
recursively, including in setup-producer arguments; records inside collections
are not. Rename members with `[SpecEvent("...")]` to resolve collisions; names
are never automatically prefixed. Duplicate names and types that reach
themselves are rejected as `AmbiguousDeclaration`. A null in a non-nullable
property-setup parameter or member, like any null in a non-nullable position,
does not fail the test: capture drops that scenario and prints a warning asking
whether the position should be nullable. A nullable member (`T?`) is
an optional and is not decomposed. Explicit setup producers take precedence over property
folding for the parameters they fill. Rust candidates opt in separately and
promise that any omitted fields may safely use `Default`.

#### 6. Async operations

Operations returning `Task`, `Task<T>`, `ValueTask`, or `ValueTask<T>` are
supported; see [`async-capture.md`](${PLUGIN_DIR}/docs/guides/async-capture.md).

#### 7. Explicit generic specializations

Give each specialization a distinct operation name and concrete type arguments:
`[SpecOperation("find_items", Spec = "example.client", TypeArguments = new[] { typeof(Item) })]`.
Repeat the annotation for other specializations. Generic constraints still
apply; calling an undeclared specialization during capture fails. Open generic
declaring types and generic setups remain unsupported.

### Expected output

```sh
specgate capture --binding specgate.yaml --component fixture.cli_capture.add --impl csharp --out capture
```

prints `Complete(component=fixture.cli_capture.add, scenarios=…, …)` and exits 0.

### Artifacts

None from instrumentation. Capture writes a bundle; see
[`capture-a-reference.md`](${PLUGIN_DIR}/docs/guides/capture-a-reference.md).

### Failure modes

The weaver fails the capture build with a message naming the method:

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `Async capture supports operations returning Task, Task<T>, ValueTask, or ValueTask<T>` | An `async void` operation, an async setup, or an `async` method returning another awaitable type. | Return `Task` / `ValueTask` (or a `<T>` form); make setups synchronous. |
| `Iterator capture is not supported` / `Stream capture is not supported` | An iterator or async iterator. | Return a materialized collection. |
| `requires one concrete TypeArguments entry per generic method parameter` | Missing or incomplete specialization. | Supply concrete `TypeArguments` for each generic method parameter. |
| `is a generic method` / `is declared on generic type` | Generic setup or generic declaring type. | Use a nongeneric setup on a nongeneric type. |
| `must take parameters by value` | A `ref`, `out`, `in`, or pointer parameter. | Take parameters by value. |
| `annotation without a Spec component` | `[SpecOperation]` without `Spec`. | Add `Spec = "<component>"`. |
| `the capture weaver wove no method of` | Discovery found operations but the woven build did not contain them. | Check that `sources` points at the project holding the annotated methods. |

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
