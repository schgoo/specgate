# C# async capture

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-16

## Context

Rust captures an `async fn` by carrying the construction-time parent context
inside the instrumented future and reinstalling it around every poll. C# needs
no such carrier: `AsyncLocal` flows through awaits, so a woven method that
captured its scope before its first `await` keeps that scope across thread
migration. The compiler's async stub runs the body eagerly to the first
`await`, so first execution coincides with the call, matching Rust's "recording
begins at first poll".

Discovery already registers `ValueTask` and `ValueTask<T>` operations as async
(`ComponentDocument.cs`), so rejecting them at capture would leave registered
but uncapturable operations. Rust has no `ValueTask` equivalent.

## Decision

- The weaver keeps the prologue and the synchronous-throw catch of an annotated
  operation, but replaces the normal-exit epilogue of an async stub with a call
  to a runtime observer (`ObserveTask`, `ObserveTask<T>`, `ObserveValueTask`,
  `ObserveValueTask<T>`). The observer returns a replacement awaitable that
  settles only after the span is completed, so the caller never sees a settled
  task whose span is still open.
- A span records the awaited value, never the task. `Task` and `ValueTask`
  record no result. Registered result types unwrap `Task<T>`/`ValueTask<T>`
  exactly as discovery does.
- A faulted task records the exception like a synchronous throw and rethrows it
  with its original stack.
- Cancellation maps to `conformance.abandoned`: span status `UNSET`, no
  propagation to the parent. For `Task` this is `IsCanceled`; for `ValueTask`
  it is an `OperationCanceledException`. This is the C# counterpart of Rust's
  dropped-while-`Pending` abandonment.
- Unsupported shapes fail the weave with a diagnostic naming the method: async
  setups, `async void`, async iterators, and other awaitable return types.

## Consequences

- A task that is dropped and never awaited has no detectable terminal state in
  C#. If it is still outstanding when the session finishes, `Finish` fails
  closed with `InvalidOperationException`; that is distinct from abandonment.
- `ValueTask` capture is C#-only; no cross-language parity row can cover it.
- Async `[SpecSetup]`, streams, handles, callbacks, and C# replay remain out of
  scope.

## Validation

- `test/csharp/SpecGate.Runtime.Tests/AsyncCaptureTests.cs` pins observer
  behavior, including interleaving, thread hops, cancellation, and failure.
- `test/csharp/SpecGate.Weaver.Tests/WovenAsyncCaptureTests.cs` executes woven
  async stubs for every supported shape.
- `WeaveRejectionTests.RejectsAsyncVoidOperation` pins the fail-closed diagnostic.

## Supersedes / superseded by

Extends [`async-capture-context.md`](async-capture-context.md) and
[`abandonment-terminal-state.md`](abandonment-terminal-state.md) to C#.
