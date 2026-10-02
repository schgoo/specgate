# Abandonment as a terminal state

> **Status:** accepted
> **Owner:** schgoo
> **Date:** 2026-10-02

## Context

M2 instruments async operations. A future dropped before completion is ordinary
control flow — `select!` drops its losers — unlike a synchronous scope dropped
without completing, which is always a bug, because the macro completes on every
return path. Today both reach `OperationScope::drop`, which sets
`terminal_error` and panics (`specgate-runtime/src/lib.rs:879-901`).

Detection needs no format change. The macro owns the async arm and can mark the
scope, so the runtime can tell the two cases apart on its own. The open question
is what an abandoned operation *means to the artifact*.

Recording abandonment as a fault is legal under the contract today via
`incomplete_capture` (`trace.md:292`), but `trace.md:262` requires the
containing span to have `ERROR` status. Every test using `select!` would then
produce an `ERROR` scenario and run span for behaving correctly, and abandonment
would be indistinguishable from a recording that ended with work outstanding.

Omitting the span is worse. If a reference run completes an operation and a
candidate run abandons it, that is a real behavioral difference, and a comparison
policy cannot weigh evidence the trace never recorded.

## Decision

1. Abandonment is a terminal state — neither a fault nor a completion outcome.
   The §7.6 completion states describe an operation completing *through its
   contract*; abandonment is imposed by the caller and is not part of the
   operation's declared `outcomes`. It sits beside `conformance.fault` as the
   second non-contractual termination: a fault is failure, an abandonment is
   work discarded before it reached any outcome.
2. A new event, `conformance.abandoned`, carries no value — analogous to
   `conformance.empty`.

   The name is deliberate. Rust has no language-level cancellation: dropping a
   future *is* cancellation, and a cooperative token such as Tokio's
   `CancellationToken` or `JoinHandle::abort` still reaches instrumentation as a
   plain drop. `specgate-runtime` depends on no executor, so it cannot read an
   executor's intent even where one exists. `abandoned` names what is observed;
   `cancelled` would claim intent the producer cannot establish.
3. An abandoned operation span MUST have `UNSET` status. `ERROR` is false
   because nothing failed; `OK` is false because the contract was not fulfilled.
   `UNSET` is the accurate "no determination reached".

   `UNSET` is unambiguous here because identification comes from the event, not
   the status, and because a span carrying no terminal event is already required
   to have `OK` status (`validation/trace.rs:614`).
4. Abandonment does not propagate status. An abandoned child MUST NOT force
   `ERROR` on any containing span. This is the point of the decision.
5. Abandonment is distinct from `incomplete_capture`. `conformance.abandoned`
   means the program discarded the operation during a recording that completed
   normally. `incomplete_capture` means the recording itself ended while the
   operation was outstanding, which is a capture defect and remains a fault.
6. Only async scopes may abandon. A synchronous scope dropped without completion
   remains a bug and keeps today's `terminal_error` behavior.
7. Whether an abandonment is a *difference* is comparison policy, not a format
   decision, and this record takes no position on it. `comparison.md:7-8` leaves
   equality to the comparator, and `async-capture-context.md` Decision 3 already
   holds that the recorder takes no position on concurrency semantics. The
   trade — catching a genuine regression against reporting a `select!` race that
   lands differently each run — belongs to the versioned comparison profile.

## Consequences

Required: `trace.md` §7.5 and §7.6; the event lists at
`specgate-ctsc/src/validation/model.rs:13` and `docs/ctsc/validate.py:34`;
message-string parity across both validators; a `NativeCompletion` variant and a
drop-path branch in the runtime; and the macro marking async scopes.

This composes with the unfinished-span rule added in PR #59
(`validation/trace.rs:614`) without amending it: an abandoned span carries a
terminal event, so it is already exempt from the rule that an unterminated span
must have `OK` status.

Prohibited: emitting `conformance.abandoned` from a synchronous scope; using
`incomplete_capture` to represent abandonment; and inferring from an abandonment
that a difference exists, which is the comparator's call.

Deferred: abandonment of an async `#[spec_setup]` (M3); supervisor-observed
abandonment such as a harness-level timeout; and the comparison-policy treatment
in Decision 7.

Intentionally unsupported: distinguishing cooperative cancellation from a merely
dropped future. Rust does not expose that difference at the drop boundary.

## Validation

New CTSC-native trace tests pin the `UNSET` status rule, the non-propagation
rule, and rejection of `conformance.abandoned` alongside another terminal event.
Validator parity is pinned by the existing cross-language checks. Golden matrix
rows covering abandonment land with the async capture slice that introduces
`select!`; the single-operation slice cannot abandon and adds none.

## Supersedes / superseded by

Supersedes nothing. Refines `async-capture-context.md`, whose Decision 4 lists
outstanding work as surfaced-but-unrepresentable; abandonment is removed from
that set and given an explicit representation. Settles the cancellation item in
roadmap M4.