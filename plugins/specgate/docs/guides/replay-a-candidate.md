# Replay a candidate

## Goal

Run the inputs recorded in a capture bundle against another implementation and
write a candidate trace you can compare with the reference.

## Prerequisites

- A capture bundle ([`capture-a-reference.md`](capture-a-reference.md)).
- A **Rust** candidate implementation of the same component, instrumented with
  the same component id, operation names, and input names
  ([`instrument-rust-crate.md`](instrument-rust-crate.md)).
- A binding entry for the candidate
  ([`write-a-binding.md`](write-a-binding.md)).

The reference may be Rust or C#; the candidate must be Rust.

## Steps

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

3. Compare ([`validate-and-compare.md`](validate-and-compare.md)).

### What replay does

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

### Runtime selection and custom configuration

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

## Expected output

```text
Complete(component=<id>, scenarios=<n>, operations=<n>, plans=<n>, output=candidate.otlp.json)
```

Exit code 0. On failure, `Error(<diagnostic>)` and exit code 1. These lines are
not yet a stable contract ([`../reference/cli.md`](../reference/cli.md#standard-output)).

## Artifacts

One OTLP JSON candidate trace. Scenario names match the reference.

## Failure modes

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `replay currently supports only Rust candidates; implementation language is` | `--impl` selected a C# implementation. | Replay onto a Rust implementation. |
| `candidate setup resolution failed` | Setups do not resolve uniquely to a receiver or injected parameter. | Annotate one matching producer per role. |
| `selected setup producers belong to different Cargo test targets` | No single target contains the environment. | Keep the required producers in one Cargo target. |
| `unsupported async replay scheduling:` | Recorded sibling operations overlap. | Record directly awaited calls; overlapping scheduling is unsupported. |
| `uses unsupported structured type` | An operation or setup input, or a field of a property setup, is a record that is not an opted-in property setup, or a record inside a collection or optional. | See [`../reference/limitations.md`](../reference/limitations.md). |
| `produces duplicate input name` | Flattened property-setup leaves collide; nested paths are not prefixed. | Rename one field with `#[spec_event(name = "...")]`. |
| `recursive property setup type` | An opted-in property-setup type reaches itself. | Remove the setup opt-in from one record in the cycle. |
| `already exists; pass --force to overwrite it` | The output file exists. | Pass `--force` or another `--out`. |

## Verify

`specgate validate linked` prints `valid: linked candidate.otlp.json` and exits
0; then `specgate compare` ([`validate-and-compare.md`](validate-and-compare.md)).
