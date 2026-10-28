# Port a component

## Goal

Show that a new implementation of a component behaves like the existing one,
including across languages: capture the original once, replay its recorded
inputs against the port, and compare.

## Prerequisites

- The original implementation, instrumented and bound
  ([Rust](instrument-rust-crate.md) or [C#](instrument-csharp-assembly.md);
  [`write-a-binding.md`](write-a-binding.md)).
- The port, written in **Rust**, instrumented with the same component id,
  operation names, input names, and type names.
- Both implementations listed under one component in the binding, for example
  labels `csharp` and `rust`.
- The boundary chosen with the
  [quick path's step 0](README.md#step-0-choose-the-boundary-before-instrumenting):
  the requested API is annotated in both languages, and the original's
  existing tests call it.

Replay supports only Rust candidates. It covers public free functions,
setup-backed methods borrowing `&self` or `&mut self`, directly awaited async
calls, and explicit generic specializations. Operation and setup construction
inputs must be primitives, recursively supported collections and optionals, or
opted-in property setups. The full list is in
[limitations](../reference/limitations.md).

## Steps

1. Capture the original: [smoke]

   ```sh
   specgate capture --binding specgate.yaml --component <id> --impl csharp --out reference
   specgate validate bundle reference
   ```

2. Replay its recorded inputs against the port: [smoke]

   ```sh
   specgate replay reference --binding specgate.yaml --impl rust --out candidate.otlp.json
   ```

3. Validate and compare with the original's registry: [smoke]

   ```sh
   specgate validate linked candidate.otlp.json reference/registry.ctsc.json
   specgate compare reference/reference.otlp.json candidate.otlp.json --registry reference/registry.ctsc.json
   ```

4. Fix each `mismatch` and repeat steps 2–3. Recapture only when the original
   or its tests change.

### Why replay, not two captures

`compare` pairs scenarios by name, and scenario names are test names. Two
independent captures of different test suites, such as a Rust suite and a C#
suite, share no names, so comparing them reports every scenario as missing.
Replay runs the original's scenarios, under the original's names, against the
port.

### Worked example: free function

From the repository root, with `specgate` on `PATH`, this replays the
checked-in C# reference of `fixture.stateless_add` against its Rust
implementation: [manual]

```sh
specgate replay test/behavior/cases/fixture.stateless_add/expected --binding test/behavior/specgate.yaml --impl rust --out candidate.otlp.json
specgate validate linked candidate.otlp.json test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
specgate compare test/behavior/cases/fixture.stateless_add/expected/reference.otlp.json candidate.otlp.json --registry test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
```

### Worked example: setup-backed method

`fixture.setup_with_params` instruments a stateful method in both languages and
reuses an existing C# test, without a scenario wrapper.

C# reference (`csharp/SetupWithParams.cs`): the setup builds the receiver, and
the method is the operation.

```csharp
[SpecSetup("increment", Spec = "fixture.setup_with_params")]
public static Counter Make([SpecInput("initial")] int initial) =>
    new() { Count = initial };

[SpecOperation("increment", Spec = "fixture.setup_with_params")]
public void Increment()
{
    Count++;
    _ = ObserveCount(Count);
}
```

The existing C# test (`csharp/tests/StatefulBehaviorTests.cs`) calls the real
API:

```csharp
SetupWithParams.Counter parameterized = SetupWithParams.Counter.Make(9);
parameterized.Increment();
Assert.Equal(10, parameterized.Count);
```

Capture folds the setup input into the operation, so the reference records
`increment` with input `initial = 9` and its nested `observe_count` call.

The Rust candidate (`rust/setup_with_params.rs`) declares the same setup,
operation, and input names:

```rust
#[spec_setup("increment", spec = "fixture.setup_with_params")]
pub fn make_counter(initial: i32) -> Counter {
    Counter { count: initial }
}

impl Counter {
    #[spec_operation("increment", spec = "fixture.setup_with_params")]
    pub fn increment(&mut self) {
        self.count += 1;
        observe_count(self.count);
    }
}
```

Replay calls `make_counter(9)` with the recorded value, then `increment` on the
result. No fixture name is passed, and neither side hard-codes the scenario's
data. From the repository root: [manual]

```sh
specgate replay test/behavior/cases/fixture.setup_with_params/expected --binding test/behavior/specgate.yaml --impl rust --out candidate.otlp.json
specgate validate linked candidate.otlp.json test/behavior/cases/fixture.setup_with_params/expected/registry.ctsc.json
specgate compare test/behavior/cases/fixture.setup_with_params/expected/reference.otlp.json candidate.otlp.json --registry test/behavior/cases/fixture.setup_with_params/expected/registry.ctsc.json
```

```text
Complete(component=fixture.setup_with_params, scenarios=1, operations=1, plans=1, output=candidate.otlp.json)
valid: linked candidate.otlp.json
policy: ctsc.strict/0.1.0
reference: test/behavior/cases/fixture.setup_with_params/expected/reference.otlp.json
candidate: candidate.otlp.json
equivalent: true
```

The golden matrix replays this case on every `just check`.

If the setup instead took a record, such as a polygon, opt the record in as a
property setup so replay deconstructs it into its fields (see the Rust and C#
instrumentation guides), or give it an explicit setup producer. Deconstruction
is one level deep: a polygon whose `points` field is a list of `Point` records
still needs a producer. A record input with neither fails with
`uses unsupported structured type` or `uses unsupported structured replay type`
(the `structured-value` limitation). Report that limitation and ask before
changing the boundary.

## Expected output

[manual] For the free-function example. Lines are not yet a stable contract
([`../reference/cli.md`](../reference/cli.md#standard-output)).

```text
Complete(component=fixture.stateless_add, scenarios=1, operations=2, plans=1, output=candidate.otlp.json)
valid: linked candidate.otlp.json
policy: ctsc.strict/0.1.0
reference: test/behavior/cases/fixture.stateless_add/expected/reference.otlp.json
candidate: candidate.otlp.json
equivalent: true
```

Every command exits 0.

## Artifacts

- `reference/` — the original's bundle ([`../reference/bundle.md`](../reference/bundle.md)).
- `candidate.otlp.json` — the port's trace.

## Failure modes

| Symptom | Cause | Fix |
| --- | --- | --- |
| `mismatch scenario["…"]: expected present, actual <missing>` | You compared two captures instead of replaying. | Follow steps 2–3. |
| `mismatch <path>: expected <x>, actual <y>` | The port behaves differently. | Fix the port. |
| Replay `Error(...)` naming an operation or type | Names or types differ from the original's registry, or a replay limitation applies. | Match the names; see [`troubleshoot-capture.md`](troubleshoot-capture.md#replay). |
| `replay currently supports only Rust candidates; implementation language is` | The port is C#. | Not supported yet. |
| `uses unsupported structured type` or `uses unsupported structured replay type` | An operation or setup construction input is structured; replay cannot rebuild it yet. | Report the limitation and ask before changing the boundary. Do not replace the input with a fixture name. |

## Verify

`specgate compare` prints `equivalent: true` and exits 0. [smoke]

## Source of truth

- [`replay-a-candidate.md`](replay-a-candidate.md),
  [`validate-and-compare.md`](validate-and-compare.md)
- [`../ctsc/comparison.md`](../ctsc/comparison.md) §7.2 — scenario matching.
- `rust/crates/specgate-cli/src/replay/`
- `scripts/package-smoke.ps1` — the release smoke runs capture, replay, and
  compare end to end.
