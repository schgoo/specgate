# Top-level async abandonment is unsupported

> **Status:** accepted
> **Owner:** SpecGate maintainers (user-ratified)
> **Date:** 2026-10-09

## Context

Environment-driven capture writes its sidecar incrementally and has no
end-of-test signal, as described in "Provisional capture sidecars" in
[`../specgate-ctsc-migration.md`](../specgate-ctsc-migration.md). Every
snapshot closes still-outstanding operations with an `incomplete_capture`
target fault at `Status::Error` so the file is always well formed.

When an async operation is abandoned — dropped while still `Pending` — the
abandonment is recorded in memory, but only a later operation boundary rewrites
the sidecar. If the abandoned operation is *top level* and no further annotated
operation runs, no boundary follows. The provisional `incomplete_capture`
placeholder is never repaired and folds through `has_error`, so a passing
scenario is recorded as `run: error` and `scenario: error`.

Abandonment nested inside an enclosing operation is unaffected: the enclosing
operation completes, re-persists, and the artifact is correct. That is the case
the `fixture.async_abandon` golden covers.

This narrows [`abandonment-terminal-state.md`](abandonment-terminal-state.md)
without changing its semantics: abandonment still records
`conformance.abandoned` with `UNSET` status, and `incomplete_capture` remains a
capture defect distinct from abandonment. What changes is that a recording
which never reaches a persisting boundary is dropped rather than emitted.

`OperationScope::drop` cannot repair this. Persisting from that destructor is
prohibited by [`native-capture-finalization.md`](native-capture-finalization.md),
which requires it to perform only infallible, in-memory cleanup.

## Decision

1. **Top-level async abandonment is an unsupported capture shape.** SpecGate
   does not attempt to produce a correct artifact for an annotated async
   operation that is abandoned with no enclosing and no subsequent annotated
   operation.
2. **It fails loudly rather than silently, and only for what it affects.** When
   a decoded sidecar contains an `incomplete_capture` target fault,
   `specgate capture` drops that scenario and records it as a failed test whose
   summary names the outstanding operation, the likely causes, and the supported
   workaround. It never emits a trace that reports `error` for a scenario whose
   test passed. The cost is bounded to that scenario: sibling components
   enumerated in the same execution pass still bundle normally, and the command
   fails only when a *requested* component is left with no scenario at all — in
   which case the diagnostic names the component and carries the dropped
   scenario's summary. Strict capture, used by golden generation, still fails
   the whole batch, so a fixture that cannot be finalized can never be hidden.
3. **Detection keys on the fact, not the cause.** A failing test's sidecar is
   already discarded before decoding, so any `incomplete_capture` reaching the
   decoded capture came from a test that reported success. More than one cause
   produces it — a top-level abandonment, and a target that calls
   `std::process::exit` inside an open operation. Every such cause leaves a
   recording that could not be finalized, so all are dropped alike. No
   exit-code heuristic or async-versus-sync inspection is used, and the
   diagnostic describes the observable fact rather than asserting a cause.
4. **The supported workaround is nesting.** Enclosing the abandoned operation
   in an annotated operation that completes produces a correct artifact today.
   The diagnostic states this.
5. **`OperationScope::drop` keeps its existing contract without exception.** No
   persistence, no diagnostics, no new `Stage` variant, and no change to the
   published `specgate-runtime` surface.

## Rejected alternative: session-scoped finalization

A thread-local session guard whose own destructor performs one final
persistence would produce a correct artifact. It was rejected as
disproportionate to the case it serves.

Recording why, so it is not re-proposed without new information:

- Rust offers only three end-of-scope hooks — a value's `Drop`, a thread-local
  destructor, and `atexit`. The destructor ban excludes the first; a
  process-exit hook is incompatible with the parallel test execution tracked in
  [issue #70](https://github.com/schgoo/specgate/issues/70) and needs a new
  dependency. A thread-local destructor is still a destructor, so this does not
  remove the hazard class — it relocates it.
- Any explicit, non-destructor finalization point requires a caller that does
  not exist. The only candidate is a mandatory test-level annotation, which is
  rejected: SpecGate captures behavior through *ordinary* tests, per
  [`../digests/llm.md`](../digests/llm.md) and `specgate/src/lib.rs`.
- The mechanism would not be universally reliable either. A thread whose
  destructors never run is not finalized, so the limitation would persist in a
  less visible form.

If top-level abandonment later proves common in practice, reopen this with
usage evidence. Capture finalization semantics are human-owned.

## Consequences

- A capture containing top-level abandonment fails with an actionable error
  instead of producing a misleading artifact. The capability is narrowed; the
  recorded behavior is never wrong.
- `incomplete_capture` retains exactly one meaning in a decoded sidecar: the
  recording could not be finalized and must not be trusted.
- `--test-threads=1` in `specgate-cli/src/capture/execution.rs` is a
  **correctness** constraint, not only a determinism one. Capture sessions on
  one process share a single resolved scenario identity and sidecar path, so
  concurrent capture threads would overwrite one another. Issue #70 tracks the
  per-session identity work required before it can be relaxed.

## Validation

- A CLI unit test asserts that a decoded sidecar carrying an
  `incomplete_capture` fault is rejected, and that the diagnostic names the
  outstanding operation.
- An end-to-end CLI test runs `specgate capture` over a fixture that exits the
  process inside an open operation, asserting the capture is rejected. This
  preserves the issue #55 coverage that fixture was added for.
- The existing `fixture.async_abandon` golden continues to pass unchanged,
  pinning that nested abandonment remains supported.