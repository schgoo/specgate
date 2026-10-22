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

Replay supports only Rust candidates, and only free, synchronous functions
without setups ([limitations](../reference/limitations.md)).

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

### Worked example

From the repository root, with `specgate` on `PATH`, this replays the
checked-in C# reference of `fixture.stateless_add` against its Rust
implementation: [manual]

```sh
specgate replay test/behavior/cases/fixture.stateless_add/expected --binding test/behavior/specgate.yaml --impl rust --out candidate.otlp.json
specgate validate linked candidate.otlp.json test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
specgate compare test/behavior/cases/fixture.stateless_add/expected/reference.otlp.json candidate.otlp.json --registry test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json
```

## Expected output

[manual] Lines are not yet a stable contract
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

## Verify

`specgate compare` prints `equivalent: true` and exits 0. [smoke]

## Source of truth

- [`replay-a-candidate.md`](replay-a-candidate.md),
  [`validate-and-compare.md`](validate-and-compare.md)
- [`../ctsc/comparison.md`](../ctsc/comparison.md) §7.2 — scenario matching.
- `rust/crates/specgate-cli/src/replay/`
- `scripts/package-smoke.ps1` — the release smoke runs capture, replay, and
  compare end to end.
