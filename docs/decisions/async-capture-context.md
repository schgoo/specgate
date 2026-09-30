# Async capture context

> **Status:** accepted
> **Owner:** schgoo
> **Date:** 2026-09-30

## Context

Native capture state in `specgate-runtime` is thread-local. Capture therefore
rejects async operations before polling, because the state cannot safely cross
an `.await`. Roadmap M1 and M2 and issue #2 all depend on replacing it.

The framing that thread-local storage is merely an implementation detail is
wrong: the real constraint is that a recording is scoped to a *test*, not to a
thread. Note also that the CLI harness already runs one process per test
(`capture.rs:663-709`, `--test-threads=1`), so per-test isolation exists at
process granularity today — the thread-local is not what blocks parallel test
execution.

## Decision

1. A recording is scoped to a test, not a thread. The recording handle lives
   inside the future and captures its parent at construction time, not by
   lookup at poll time. This survives thread migration. Inline combinators
   (`join!`, `select!`, `FuturesUnordered`) need no special handling, because
   each future carries its own handle; spawned tasks use the identical
   mechanism.
2. Concurrent branches buffer their spans and emit complete subtrees at the
   join. Nothing interleaves writes into shared state during execution.
3. The recorder takes no position on concurrency it cannot determine
   structurally. It does not synthesize parallel regions from timestamp
   overlap, and it does not refuse overlap. `trace.md` §6.4 is explicit that
   timestamp overlap alone does not declare parallel semantics. Ordering
   semantics belong to the versioned comparison profile, not the recorder.
4. Three cases cannot be represented faithfully. Each is surfaced in the
   artifact, never only on the console:
   - work still outstanding when the test ends — the recording is marked
     incomplete;
   - an annotated operation reached through a raw `std::thread::spawn` — the
     span is recorded under the scenario, with its parent noted as
     undetermined, because the session is findable process-globally but the
     parent operation is not recoverable;
   - concurrent work whose spans do not overlap, such as a `join!` fast path —
     this records as sequential, and is a written-down known limitation rather
     than something the recorder guesses at.
5. Span identifiers are unchanged. They are already deterministic per test, and
   comparison never reads them (`comparison.rs:428-432`), so changing them
   would churn every golden for a readability gain only.

## Consequences

Required: the completion-tracking hole in trace validation is fixed as its own
separate change — `trace.md` §7.3 and §7.6 make unit completion a status with
no emitted event, and `validation/trace.rs` currently requires at most one
completion but never at least one, so an unfinished span validates clean.

Prohibited: inferring parallel semantics from timing.

Deferred: concurrency across test *processes*, which is separate and cheap; and
comparison changes, which stay out of scope.

Intentionally unsupported: faithful recording of the three cases above.

Scope of this record is concurrency within a single test process. The work
lands as at least two pull requests, each under roughly 500 lines.

## Validation

`validation/trace.rs:394-449` validates parent-kind rules and parallel-span
enclosure but deliberately does not check sibling overlap — that absence is
load-bearing, and it is what lets overlapping siblings validate cleanly while
only `ctsc.strict/0.1.0` objects.

New CTSC-native trace and capture tests pin per-test scoping, buffered subtree
emission at joins, and each of the three surfaced limitations.

Golden matrix rows currently assert that async components are discovery-only;
those rows change when capture stops rejecting async.

## Supersedes / superseded by

Supersedes nothing. Relates to roadmap M1 and M2, and to issue #2, whose
problem statement is correct but whose proposed solution predates CTSC span
parentage.
