# Async capture

## Goal

Capture async operations correctly in Rust and C#, and know which shapes are
not captured.

## Prerequisites

- An instrumented implementation
  ([Rust](instrument-rust-crate.md) or [C#](instrument-csharp-assembly.md)).
- Familiarity with [`capture-a-reference.md`](capture-a-reference.md).

## Steps

### Rust

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

### C#

1. Annotate methods returning `Task`, `Task<T>`, `ValueTask`, or `ValueTask<T>`:

   ```csharp
   [SpecOperation("fetch", Spec = "fixture.cli_capture.fetch")]
   public static async Task<string> Fetch([SpecInput("url")] string url) =>
       await Task.FromResult($"response from {url}");
   ```

2. Await every annotated task before the test ends.
3. Keep setups synchronous.

## Expected output

The same `Complete(...)` line and bundle as synchronous capture. [it]

## Artifacts

A normal bundle. Each async operation is one operation span with the awaited
value as its result.

## Behavior

| Behavior | Rust | C# |
| --- | --- | --- |
| Supported shapes | `async fn` operations | `Task`, `Task<T>`, `ValueTask`, `ValueTask<T>` |
| Span opens | At the future's first poll; a future never polled records nothing | When the method is called |
| Recorded result | The output value | The awaited value; `Task` and `ValueTask` record none |
| Thread migration | A future built inside a recording operation keeps its parent when polled on another thread | Supported |
| Failure | `Err` / panic as for sync code | A faulted task is recorded like a throw and rethrown |
| Abandonment | Dropped while pending: `conformance.abandoned`, status `UNSET`, not propagated to the parent | Cancellation (`IsCanceled`, or `OperationCanceledException` from a `ValueTask`): same marker and status |
| Never finished | Top-level abandonment, or work still open at test end: the scenario is dropped with a diagnostic | An unawaited task still outstanding when the test's session finishes: the open span is recorded as a fault and `InvalidOperationException` is thrown, so that test contributes no scenario |
| Rejected | Async setups | Async setups, `async void`, async iterators, `async` methods returning other awaitables |
| Replay | Not supported | Not supported (no C# replay) |

Wrap a future you intend to abandon inside an enclosing annotated operation
that completes; the abandonment is then recorded on the inner span
([`decisions/top-level-abandonment-unsupported.md`](../decisions/top-level-abandonment-unsupported.md)).

Not covered by tests: handing the top-level annotated future to a
multi-threaded executor, or reaching an operation through a raw
`std::thread::spawn`
([`decisions/async-capture-context.md`](../decisions/async-capture-context.md)).
Avoid both in capture tests.

## Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `was still open when the capture ended` | A Rust operation never completed, for example a top-level future dropped while pending. | Await it, or enclose it in an annotated operation that completes. |
| `native capture cannot instrument an async setup` | Rust async `#[spec_setup]`. | Make the setup synchronous. |
| `Async capture supports operations returning Task, Task<T>, ValueTask, or ValueTask<T>` | C# `async void`, async setup, or an `async` method returning another awaitable. | Return a supported type; make setups synchronous. |
| `is async; replay supports only synchronous operations` | Replay onto an async candidate. | Not supported yet; compare captures instead. |

## Verify

`specgate validate bundle <capture-dir>` exits 0, and the trace contains one
operation span per awaited call. [it]

## Source of truth

- [`decisions/async-capture-context.md`](../decisions/async-capture-context.md)
- [`decisions/abandonment-terminal-state.md`](../decisions/abandonment-terminal-state.md)
- [`decisions/top-level-abandonment-unsupported.md`](../decisions/top-level-abandonment-unsupported.md)
- [`decisions/csharp-async-capture.md`](../decisions/csharp-async-capture.md)
- [`../ctsc/trace.md`](../ctsc/trace.md) — `conformance.abandoned`.
- Fixtures: `test/behavior/cases/fixture.async_*`.
