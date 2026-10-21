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

1. Replay: [smoke]

   ```sh
   specgate replay <capture-dir> --binding specgate.yaml --impl <label> --out candidate.otlp.json
   ```

   - The component comes from the bundle's manifest.
   - Omit `--impl` when the component has one implementation.
   - Omit `--out` to write
     `<capture-dir>/candidate-<impl>-<capture-dir name>.otlp.json`.
   - Add `--force` to overwrite an existing output file.

2. Validate the candidate trace against the bundle's registry: [smoke]

   ```sh
   specgate validate linked candidate.otlp.json <capture-dir>/registry.ctsc.json
   ```

3. Compare ([`validate-and-compare.md`](validate-and-compare.md)).

### What replay does

1. Validates the bundle and decodes its scenarios.
2. Discovers the candidate and links each captured operation to exactly one
   candidate operation by component, operation name, and input names, checking
   that types are compatible.
3. Generates a runner package in a scratch directory that calls the candidate
   with the recorded inputs, scenario by scenario, under native capture.
4. Writes the resulting trace with deterministic ids and timestamps.

Replay does not run your tests; it calls your operations directly.

## Expected output

[smoke]

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
| `is setup-backed; replay does not yet construct setups` | The component has `#[spec_setup]` operations. | Not supported yet; compare captures instead. |
| `is a method; replay supports only free functions` | The operation is a method. | Not supported yet. |
| `is async; replay supports only synchronous operations` | The operation is `async`. | Not supported yet. |
| `uses unsupported structured type` | An input or result type replay cannot construct. | See [`../reference/limitations.md`](../reference/limitations.md). |
| `already exists; pass --force to overwrite it` | The output file exists. | Pass `--force` or another `--out`. |

## Verify

`specgate validate linked` prints `valid: linked candidate.otlp.json` and exits
0; then `specgate compare` ([`validate-and-compare.md`](validate-and-compare.md)).
[smoke]

## Source of truth

- `rust/crates/specgate-cli/src/replay/` — `link.rs`, `runner.rs`,
  `limitation.rs`.
- [`../reference/limitations.md`](../reference/limitations.md)
- [`../contributor/component-identity-and-replay.md`](../contributor/component-identity-and-replay.md)
  (design background; some code anchors are historical)
