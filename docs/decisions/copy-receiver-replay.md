# Replay of Copy by-value receivers

> **ADR:** ADR-0027
> **Status:** proposed; not implemented
> **Owner:** @schgoo (approval requested)
> **Date:** 2026-11-18
> **Tracking:** [#157](https://github.com/schgoo/specgate/issues/157),
> [roadmap #159](https://github.com/schgoo/specgate/issues/159)

## Context

**Rust can pass a `Copy` object by value without consuming the object replay
needs for later calls.**

Why it matters: existing by-value methods could replay without free-function
wrappers that change the annotated boundary.

- **Receiver:** the object on which a method runs. Replay uses a setup producer
  to create that object and retains it within one recorded test case.
- **Current limit:** replay supports borrowed receivers, `&self` and
  `&mut self`, but rejects methods taking ownership through `self`.
- **Safe special case:** Rust's compiler permits implicit copying of a `Copy`
  value. A by-value call can use a copy while retaining the original receiver
  for later calls.
- **Not a purity promise:** a copied value can contain shared references.
  The real method must still run once, including effects through those
  references; copying does not make the call harmless or independent.

## Minimal example

Rust excerpt; imports and the crate-root component declaration are omitted.
`replay_receiver` is a **proposed** option, not current annotation syntax:

```rust
#[derive(Clone, Copy, SpecEvent)]
pub struct Counter {
    #[spec_event]
    pub count: i32,
}

#[spec_setup("get_count")]
fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("get_count", replay_receiver = "copy")]
    pub fn get_count(self) -> i32 {
        self.count
    }
}
```

After setup with `initial = 3`, replay could call `get_count` twice and receive
`3` both times. Each call copies the retained receiver, then runs the actual
method once. Replay neither calls `Clone` nor reruns `make_counter` between calls.
The compiler must reject this opt-in if `Counter` does not implement `Copy`.

## Decision

**Requested:** approve explicit compiler-checked copying, not ownership transfer.
Proposed annotation option: `#[spec_operation("name", replay_receiver = "copy")]`.

| Boundary | Rule |
|---|---|
| Eligibility | Ordinary by-value `self` on a compiler-proven `Self: Copy`; synchronous, nongeneric first slice. |
| Selection | The explicit option declares replay intent. No change to native method signature or capture inputs. |
| Invocation | The operation-local bridge copies the current retained receiver, then invokes the real method once. |
| Proof | Generated code requires `Copy`; an invalid opt-in fails compilation before target execution. |
| No opt-in | Preserve current early planning rejection of consuming receivers. |
| Lifetime | Keep the retained receiver and guards until normal scenario teardown. |

The bridge must not call `Clone`, use unsafe bit copies, reconstruct from setup
inputs again, or infer capability from parsed derives/type names. A borrowed
mutation followed by a by-value call must copy the updated retained value.

## Consequences

- An annotation option is added; no public reconstruction trait, CTSC type,
  registry identity, runtime package substitution, or CLI option is needed.
- Opted-in non-Copy types produce a compiler diagnostic, not a late target
  failure. This differs deliberately from the unannotated planner rejection.
- Exclude `Box<Self>`/other owned receiver forms, non-Copy consumption, and
  automatic `Clone` fallback. Async/generic expansion needs separate evidence.
- Reject automatic capability inference for this slice: compiler-local proof
  is reliable; a source-text heuristic is not.

## Validation

Run real setup/capture/replay tests for repeated by-value calls, a later
borrowed call, borrowed mutation before a copy, and effects through shared
references. Compile-negative evidence covers invalid opt-in and unsupported
receiver forms; preserve the existing non-Copy planning rejection. Verify
operation-local resolution across private setup/test modules.

Implementation seams: operation argument parsing/metadata, lexical dispatch,
receiver planning, and runner invocation. Update method-shape docs; run
focused macro/planner/runner tests, generated README checks, and `just check`.

## Supersedes / superseded by

Narrowly extends the consuming-receiver prohibition in
[typed replay](typed-async-client-replay.md#candidate-owned-execution-2026-10-22).
Setup identity, sharing, and scenario isolation are unchanged.
