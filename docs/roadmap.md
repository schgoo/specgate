# SpecGate roadmap to MVP

> **Status:** living document. Tiers are owner-defined; per-item scope is
> derived from repository evidence and is revisable.
>
> **Date:** 2026-09-28
>
> **Related:** [`specgate-ctsc-migration.md`](specgate-ctsc-migration.md) for
> current limitations, [`component-identity-and-replay.md`](component-identity-and-replay.md)
> for the open model questions that gate several items below.

## Tiers

| Tier | Capability | One-line test of "done" |
|---|---|---|
| **MVP** | Rust capture, including async | A real async Rust component produces a validated, linkable reference bundle |
| **MVP+1** | C# capture | A C# component's ordinary tests produce a validated, linkable reference bundle |
| **V2** | Comparison | Two independently captured native bundles can be paired and compared |
| **V3** | Replay | A reference bundle drives a candidate and emits an independent trace |

Replay is deliberately last. Paired-native differential testing needs capture on
both sides plus comparison; replay is the convenience that removes the need for
a candidate test suite, not a prerequisite for value.

### Why Rust capture comes first

The concurrency model is the thing not to invent twice.

Thread-affine capture state is the root cause behind async rejection and the
external capture threads consumers were forced to build. Solving it once, in the
runtime SpecGate owns end to end, produces a proven model that C#
instrumentation can then implement rather than redesign.

Parallel test execution is not on that list. The CLI harness already runs one
process per test (`capture.rs:683`, `--test-threads=1`), so per-test isolation
exists at process granularity, and thread affinity is not what constrains it.
See [`decisions/async-capture-context.md`](decisions/async-capture-context.md).

Building synchronous C# capture first would mean building on thread-affine
assumptions that every documented consumer violates, and reworking it when
async arrives. **C# capture should therefore be built directly against the
concurrency model from MVP, skipping a synchronous-only stage.**

The cost of this ordering is honest and should be stated: MVP does not unblock
any consumer in the external feature request document, because all of them are
async C#. MVP+1 is what serves them.

## Where we are

| Area | State | Evidence |
|---|---|---|
| Registry discovery | Rust link-time and C# compiled-assembly reflection; byte-identical for stateless, rich-type, and setup-folding fixtures | `specgate-discovery` |
| Trace capture | Rust libtest only, synchronous only | `capture.rs:189`, migration limitations |
| C# trace capture | None - annotations are inert; no recording code exists | `csharp/SpecGate.Annotations` is the whole C# surface |
| Async capture | Still rejected before polling; the per-run operation collector is task-safe since M1, but setup-input staging is still thread-affine, no async instrumentation exists, and async setup is not instrumented at all | `lib.rs:1334`, `lib.rs:1695`, `annotations-macros/src/lib.rs:236` |
| Observations | Captured but never declared; a component emitting one cannot produce a linkable bundle | migration limitations |
| Comparison | `compare <reference-trace> <candidate-trace>` exists; fixed `ctsc.strict/0.1.0`; scenarios paired by name | `comparison.rs:185-195` |
| Replay | Synchronous public Rust free functions, lossless primitive inputs | migration limitations |

The CLI surface for all five verbs already exists. The gaps are semantic, not
structural.

---

## MVP - complete and correct Rust capture

### M1. Concurrency-safe run collection

Replace thread-local capture state with one per-run collector safe under
concurrent access.

This is the keystone: async rejection and the need for external capture threads
both trace back to capture state being reachable only from the thread that
started it. Nothing else in MVP lands cleanly before it.

**State: structurally complete, symptoms still open.** The per-run collector
exists and is `Send` (`CollectorHandle` in `specgate-runtime`), and trace
validation rejects an operation span with no terminal event. The ambient lookup
slot stays thread-local by design, and no public API yet hands a collector to
another thread, so async capture and external capture threads remain blocked
until M2 consumes the collector.

Relates to issue #2, whose problem statement is correct but whose proposed
solution predates CTSC span parentage.

The capture-context design is ratified in
[`decisions/async-capture-context.md`](decisions/async-capture-context.md),
which also covers M2.

### M2. Rust async context propagation

Operation context must survive every `Future` poll and every spawn, thread, and
channel handoff, so that parentage remains correct when execution moves between
threads.

Unblocks removing the pre-poll async rejection.

### M3. Async setup instrumentation

An async `#[spec_setup]` is not instrumented at all today, so capture rejects
the whole component up front rather than emitting a bundle without setup
inputs. Setup-folded inputs are part of the declared surface, so this is
required for correctness, not ergonomics.

This is not only a macro gap. M1 moved the *operation* recording behind a
`Send` collector handle, but left `PENDING_SETUP_INPUTS` (`lib.rs:1695`) as a
bare thread-local holding its data directly. It is the last thread-affine piece
of capture state. A setup stages its inputs for a later operation to fold in,
so that handoff breaks across threads: a setup polled on one thread stages into
that thread's map, and an operation polled on another reads an empty one and
records without the folded inputs, which linked validation then rejects as
`input names do not match registry operation`.

So M3 has a runtime prerequisite: move setup-input staging into the per-run
collector, mirroring what M1 did for operation recording. Instrumenting the
macro first would pass a single-threaded `block_on` test and stay silently
wrong under any executor that migrates the future between threads.

### M4. Concurrency semantics in the trace

Format-level decisions, not just runtime work:

- unique span identity per retry or repeated identical invocation;
- explicit parallel regions;
- cancellation representation — settled by
  [`abandonment-terminal-state`](decisions/abandonment-terminal-state.md);
- known-started but unfinished operations;
- exactly one completion or fault treatment per span.

**These are human-owned CTSC decisions.** Settle them before or early in
implementation, or the runtime work stalls against them.

### M5. Link-time observation metadata

`spec_trace!` observations are captured but never declared, because discovery
has no link-time observation metadata, so any component emitting one cannot
produce a linkable reference bundle. Fixtures currently work around this by
expressing intermediate behavior as nested public operations.

This is an MVP-level hole in capture that is easy to overlook because no
current fixture trips it.

### M6. Golden matrix update

The matrix currently **asserts** that components declaring async operations are
discovery-only. Fixing capture therefore requires matrix changes, new async
fixtures, and regenerated goldens.

---

## MVP+1 - C# capture

C# today is **annotations plus reflection only**. The entire C# source surface
is one 4 KB file of inert attributes (`SpecOperation`, `SpecSetup`,
`SpecInput`, `SpecEvent`, `SpecException`) plus `Option<T>` and `Result<T,E>`
for metadata discovery. There is no recording code: no runtime, no weaver, no
buffer. The former weaver and runtime were removed during the CTSC migration.

This is smaller than a rebuild, because everything around it already exists and
is proven:

| Piece | State |
|---|---|
| Annotations | Done, stable |
| Discovery to registry | Done, reflection over the built assembly |
| Registry parity with Rust | Byte-identical on stateless, rich-type, and setup-folding fixtures |
| Fixtures, including async | Done |
| Ordinary behavior tests over those fixtures | Done - eight test files |
| Observation of those tests | **Missing** |

The capture model is "capture ordinary tests at native operation boundaries."
In C# those ordinary tests already exist and pass. The missing piece is
precisely one thing: something that observes them while they run and emits CTSC
spans.

Scope:

1. A C# recording runtime implementing the MVP concurrency model, with logical
   context across `Task` boundaries via `AsyncLocal`, `Activity`, or
   equivalent.
2. A test-run collector that emits a validated CTSC bundle.
3. Instrumentation at annotated boundaries. The mechanism - weaving, source
   generation, or interception - is an open implementation choice.
4. Capture support for non-Rust binding targets, which `capture.rs` currently
   rejects outright.

**Epic #48 needs rescoping.** It and its children #44, #45, #46, #47 are
currently scoped synchronous, which this ordering supersedes.

## V2 - comparison

### V2.1 Scenario identity  *(blocker)*

Scenarios are paired **by name**, and a scenario name is whatever the test
framework called the function. Replay hides this because the candidate trace
inherits reference names. **Two independently captured bundles do not pair**
unless their test function names coincide, which they will not across languages
or across a rewrite.

This is open question 1, and it is a hard blocker for the entire tier. CTSC
already treats scenario matching as a policy dimension, so the extension point
exists and is unused.

### V2.2 Comparison policy beyond fixed strict

`ctsc.strict/0.1.0` rejects multiple-run selection, overlapping sequential
children, and duplicate parallel branch identities as unsupported or ambiguous.

Concurrency makes this sharper: once MVP emits genuinely parallel traces, event
order is no longer total, and a strict total-order comparison will report
differences that are not differences.

Note that the external feature request document places partial-order comparison
**outside** SpecGate. If that holds, SpecGate must at minimum express the
partial order faithfully enough for an external comparator to consume.

### V2.3 Cross-language pairing

Rust reference against C# candidate. Depends on V2.1 and on M6.

---

## V3 - replay

| Item | Note |
|---|---|
| Setup-backed methods and structured values | Today: public free functions, lossless primitives only |
| Replay unit | Open question 2 - whether replay should stop asking for a component |
| Artifact granularity | Open question 3 - one top-level component per bundle as an invariant |
| C# replay | Not implemented |
| Async replay | Depends on all of MVP |

---

## Open questions by tier

| Question | Gates |
|---|---|
| 1 - scenario identity | **V2 blocker** |
| 2 - what replay consumes | V3 |
| 3 - artifact granularity | V3, capture profiles |
| 4 - registry identity versus content | V2, V3 - comparison across differently resolved surfaces |
| 5 - process boundaries | Post-V3 |
| 6 - declared external identity | Not on the critical path |
| 7 - boundary contracts | Post-V3 |

Capture profiles are preserved unmerged on `wip/ctsc-capture-profiles` and stay
parked until questions 1 through 3 are resolved.

---

## Triage needed

- 26 open issues, several predating the CTSC migration and describing removed
  subsystems. Issue #2 is the clearest case: right problem, obsolete solution.
- Epic #48 and children #44-#47 are scoped to synchronous C# capture and need
  rescoping against MVP+1.
- The external feature request document cites `specgate-harness` paths and a C#
  weaver and runtime that no longer exist. Its evidence and requirements remain
  valid; its citations do not.