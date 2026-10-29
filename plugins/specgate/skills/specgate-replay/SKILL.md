---
name: specgate-replay
description: "Replays a captured SpecGate bundle against a Rust candidate implementation and walks through porting a component from C# or Rust to Rust. Use when running specgate replay, checking that a Rust port matches the original, or when replay reports an unsupported operation or input."
---

# Replay and port with SpecGate

Part of the SpecGate agent plugin 0.6.0. It covers using SpecGate in the user's project, not developing SpecGate itself.

## Path resolution

`${PLUGIN_DIR}` is the plugin root, the directory that holds `.claude-plugin/plugin.json`.

- **Claude Code:** `${PLUGIN_DIR}` is `${CLAUDE_PLUGIN_ROOT}`.
- **Other agents:** this file is `skills/specgate-replay/SKILL.md`, so the plugin root is two directories up.

`${PLUGIN_DIR}/docs/` holds every SpecGate guide and reference page. Links to `https://github.com/schgoo/specgate/…/v0.6.0/` show SpecGate's own sources and examples at this release. Read them as examples; do not adopt SpecGate's contributor workflow (`just` recipes, the golden matrix) in the user's project.

## Replay a candidate

### Goal

Run the inputs recorded in a capture bundle against another implementation and
write a candidate trace you can compare with the reference.

### Prerequisites

- A capture bundle ([`capture-a-reference.md`](${PLUGIN_DIR}/docs/guides/capture-a-reference.md)).
- A **Rust** candidate implementation of the same component, instrumented with
  the same component id, operation names, and input names
  ([`instrument-rust-crate.md`](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md)).
- A binding entry for the candidate
  ([`write-a-binding.md`](${PLUGIN_DIR}/docs/guides/write-a-binding.md)).

The reference may be Rust or C#; the candidate must be Rust.

### Steps

1. Replay:

   ```sh
   specgate replay <capture-dir> --binding specgate.yaml --impl <label> --out candidate.otlp.json
   ```

   - The component comes from the bundle's manifest.
   - Omit `--impl` when the component has one implementation.
   - Omit `--out` to write
     `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`.
   - Add `--force` to overwrite an existing output file.

2. Validate the candidate trace against the bundle's registry:

   ```sh
   specgate validate linked candidate.otlp.json <capture-dir>/registry.ctsc.json
   ```

3. Compare ([`validate-and-compare.md`](${PLUGIN_DIR}/docs/guides/validate-and-compare.md)).

#### What replay does

1. Validates the bundle and decodes its scenarios.
2. Discovers the candidate and links each captured operation to exactly one
   candidate operation by component, operation name, and input names, checking
   that types are compatible.
3. Generates a runner or a selected Cargo test-target entry that calls the
   candidate with recorded operation and setup construction inputs, scenario by
   scenario, under native capture. Async calls use the selected candidate engine
   or a custom candidate-owned wrapper.
4. Writes the resulting trace with deterministic ids and timestamps.

Replay does not run your tests; it calls your operations directly.
Annotated synchronous setups reconstruct receiver and injected environments.
Private/test-only producers remain in their original Cargo target. Equal
receiver producer/input identities share state within a scenario; scenario
boundaries release receivers and setup guards. Structured outcomes must match
the reference's reachable CTSC declarations. See
[`../status.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/status.md#candidate-replay-and-explicit-generic-operations)
for explicit generic specializations.

Each Cargo target used for generated discovery/replay hooks must declare
`specgate::spec_component!("…")` at its own crate root, including binary-unit
and integration-test targets. Explicit operation component arguments do not
replace that declaration. Generated builds use the original sources, support
read-only source directories, and leave configured compiler wrappers to Cargo.

#### Runtime selection and custom configuration

Replay supports Arty, Smol, and Tokio. By default it selects the only supported
direct candidate dependency, including development dependencies and renamed
crates. Several matching dependencies require an explicit `rust.runtime`
selection. If none match, add an engine or supply `rust.executor`.
The generated harness uses that candidate dependency, not a second runtime
package hidden in `specgate-runtime`. External runners preserve the resolved
engine's exact version and supported path/registry source.

Arty runs the entire replay task on an Arty worker, constructing non-`Send`
receivers and futures there. Smol uses `block_on`; Tokio uses a current-thread
runtime with enabled drivers. Arty does not supply Tokio-compatible I/O
drivers. Choose the engine your candidate actually needs.

For custom startup, point the binding at ordinary candidate code:

```yaml
rust:
  executor: crate::tests::replay_executor
```

For example, share your existing runtime-building helper with this wrapper:

```rust
pub(crate) fn replay_executor<F, Fut>(task: F) -> Result<(), String>
where
    F: FnOnce() -> Fut + Send,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let runtime = test_runtime()?; // Your configured arty::runtime::Runtime.
    let result = runtime.scheduler().block_on(async move |_cx| task().await);
    runtime.stop().map_err(|error| error.to_string())?;
    result.map_err(|error| error.to_string())?
}
```

The wrapper must call and await the factory exactly once, on the desired
executor. It owns the full replay, including receiver/setup construction and
teardown. Arty's task capabilities (`Builtins`) are explicit: a custom wrapper
can make them available through the same application-owned context mechanism
used by its ordinary tests. SpecGate does not invent a process-global Arty
context or start another engine inside a custom wrapper. Neither the factory
nor its future needs a `'static` lifetime, and the future need not be `Send`.

In a Cargo test target, the function and its parent modules must be accessible
from generated entries (`pub(crate)` is sufficient); it may be `#[cfg(test)]`.
An external runner instead needs an exported production function.
Synchronous plans use immediately ready invocation bridges without starting an
async engine, unless the binding explicitly requests an execution wrapper.

### Expected output

```text
Complete(component=<id>, scenarios=<n>, operations=<n>, plans=<n>, output=candidate.otlp.json)
```

Exit code 0. On failure, `Error(<diagnostic>)` and exit code 1. These lines are
not yet a stable contract ([`../reference/cli.md`](${PLUGIN_DIR}/docs/reference/cli.md#standard-output)).

### Artifacts

One OTLP JSON candidate trace. Scenario names match the reference.

### Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `replay currently supports only Rust candidates; implementation language is` | `--impl` selected a C# implementation. | Replay onto a Rust implementation. |
| `candidate setup resolution failed` | Setups do not resolve uniquely to a receiver or injected parameter. | Annotate one matching producer per role. |
| `selected setup producers belong to different Cargo test targets` | No single target contains the environment. | Keep the required producers in one Cargo target. |
| `unsupported async replay scheduling:` | Recorded sibling operations overlap. | Record directly awaited calls; overlapping scheduling is unsupported. |
| `uses unsupported structured type` | An operation or setup input, or a field of a property setup, is a record that is not an opted-in property setup, or a record inside a collection or optional. | See [`../reference/limitations.md`](${PLUGIN_DIR}/docs/reference/limitations.md). |
| `produces duplicate input name` | Flattened property-setup leaves collide; nested paths are not prefixed. | Rename one field with `#[spec_event(name = "...")]`. |
| `recursive property setup type` | An opted-in property-setup type reaches itself. | Remove the setup opt-in from one record in the cycle. |
| `already exists; pass --force to overwrite it` | The output file exists. | Pass `--force` or another `--out`. |

### Verify

`specgate validate linked` prints `valid: linked candidate.otlp.json` and exits
0; then `specgate compare` ([`validate-and-compare.md`](${PLUGIN_DIR}/docs/guides/validate-and-compare.md)).

## Port a component

### Goal

Show that a new implementation of a component behaves like the existing one,
including across languages: capture the original once, replay its recorded
inputs against the port, and compare.

### Prerequisites

- The original implementation, instrumented and bound
  ([Rust](${PLUGIN_DIR}/docs/guides/instrument-rust-crate.md) or [C#](${PLUGIN_DIR}/docs/guides/instrument-csharp-assembly.md);
  [`write-a-binding.md`](${PLUGIN_DIR}/docs/guides/write-a-binding.md)).
- The port, written in **Rust**, instrumented with the same component id,
  operation names, input names, and type names.
- Both implementations listed under one component in the binding, for example
  labels `csharp` and `rust`.
- The boundary chosen with the
  [quick path's step 0](${PLUGIN_DIR}/docs/guides/README.md#step-0-choose-the-boundary-before-instrumenting):
  the requested API is annotated in both languages, and the original's
  existing tests call it.

Replay supports only Rust candidates. It covers public free functions,
setup-backed methods borrowing `&self` or `&mut self`, directly awaited async
calls, and explicit generic specializations. Operation and setup construction
inputs must be primitives, recursively supported collections and optionals, or
opted-in property setups. The full list is in
[limitations](${PLUGIN_DIR}/docs/reference/limitations.md).

### Steps

1. Capture the original:

   ```sh
   specgate capture --binding specgate.yaml --component <id> --impl csharp --out reference
   specgate validate bundle reference
   ```

2. Replay its recorded inputs against the port:

   ```sh
   specgate replay reference --binding specgate.yaml --impl rust --out candidate.otlp.json
   ```

3. Validate and compare with the original's registry:

   ```sh
   specgate validate linked candidate.otlp.json reference/registry.ctsc.json
   specgate compare reference/reference.otlp.json candidate.otlp.json --registry reference/registry.ctsc.json
   ```

4. Fix each `mismatch` and repeat steps 2–3. Recapture only when the original
   or its tests change.

#### Why replay, not two captures

`compare` pairs scenarios by name, and scenario names are test names. Two
independent captures of different test suites, such as a Rust suite and a C#
suite, share no names, so comparing them reports every scenario as missing.
Replay runs the original's scenarios, under the original's names, against the
port.

#### Worked example: free function

From the repository root, with `specgate` on `PATH`, this replays the
checked-in C# reference of `fixture.stateless_add` against its Rust
implementation:

```sh
specgate replay test/behavior/cases/fixture.stateless_add/expected --binding test/behavior/specgate.yaml --impl rust --out candidate.otlp.json
specgate validate linked candidate.otlp.json test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
specgate compare test/behavior/cases/fixture.stateless_add/expected/reference.otlp.json candidate.otlp.json --registry test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
```

#### Worked example: setup-backed method

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
data. From the repository root:

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
instrumentation guides), or give it an explicit setup producer. Nested
opted-in records, including those passed to a producer, are deconstructed
recursively, but records inside collections or optionals are not. A polygon
whose `points` field is a list of `Point` records therefore cannot be replayed
through either path, while one with opted-in `origin: Point` and `size: Size`
fields can. A record input that
cannot be rebuilt fails with `uses unsupported structured type` (the
`structured-value` limitation). Report that limitation and ask before changing
the boundary.

### Expected output

For the free-function example. Lines are not yet a stable contract
([`../reference/cli.md`](${PLUGIN_DIR}/docs/reference/cli.md#standard-output)).

```text
Complete(component=fixture.stateless_add, scenarios=1, operations=2, plans=1, output=candidate.otlp.json)
valid: linked candidate.otlp.json
policy: ctsc.strict/0.1.0
reference: test/behavior/cases/fixture.stateless_add/expected/reference.otlp.json
candidate: candidate.otlp.json
equivalent: true
```

Every command exits 0.

### Artifacts

- `reference/` — the original's bundle ([`../reference/bundle.md`](${PLUGIN_DIR}/docs/reference/bundle.md)).
- `candidate.otlp.json` — the port's trace.

### Failure modes

| Symptom | Cause | Fix |
| --- | --- | --- |
| `mismatch scenario["…"]: expected present, actual <missing>` | You compared two captures instead of replaying. | Follow steps 2–3. |
| `mismatch <path>: expected <x>, actual <y>` | The port behaves differently. | Fix the port. |
| Replay `Error(...)` naming an operation or type | Names or types differ from the original's registry, or a replay limitation applies. | Match the names; see [`troubleshoot-capture.md`](${PLUGIN_DIR}/docs/guides/troubleshoot-capture.md#replay). |
| `replay currently supports only Rust candidates; implementation language is` | The port is C#. | Not supported yet. |
| `uses unsupported structured type` | An operation or setup construction input, or a property-setup field, is a record that is not an opted-in property setup, or a record sits inside a collection or optional. | Opt the record in as a property setup if its fields are primitives, collections, optionals, or opted-in records. Otherwise report the limitation and ask before changing the boundary. Do not replace the input with a fixture name. |
| `uses unsupported structured replay type mapping` | The port's structured result or error type, or a type reachable from it, does not match the reference's CTSC declaration. Replay has no explicit type mappings. | Make the port's type declare the same semantic name, fields, and variants (rename fields with `#[spec_event(name = "…")]`). If the shapes must differ, report the limitation. |

### Verify

`specgate compare` prints `equivalent: true` and exits 0.
