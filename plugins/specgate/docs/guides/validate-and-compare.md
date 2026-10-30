# Validate and compare

## Goal

Check that capture and replay artifacts are well formed, then decide whether a
candidate behaves like the reference.

## Prerequisites

- A capture bundle ([`capture-a-reference.md`](capture-a-reference.md)).
- A candidate trace, from replay
  ([`replay-a-candidate.md`](replay-a-candidate.md)) or another capture.

## Steps

1. Validate the bundle:

   ```sh
   specgate validate bundle <capture-dir>
   ```

2. Validate the candidate trace against the bundle's registry:

   ```sh
   specgate validate linked candidate.otlp.json <capture-dir>/registry.ctsc.json
   ```

3. Compare with the registry:

   ```sh
   specgate compare <capture-dir>/reference.otlp.json candidate.otlp.json --registry <capture-dir>/registry.ctsc.json
   ```

### Validation levels

| Command | Use it for |
| --- | --- |
| `validate registry <registry.json>` | A registry on its own. |
| `validate trace <trace>` | Trace Core rules only, without a registry. |
| `validate linked <trace> <registry>` | A trace's values against the registry's declared types. |
| `validate bundle <capture-dir>` | Manifest, digests, registry, and trace together. |

Add `--import <registry.json>` (repeatable) to `registry` and `linked` when the
root registry imports others.

### How compare decides

`compare` uses the fixed `ctsc.strict/0.1.0` policy
([`../ctsc/comparison.md`](https://github.com/schgoo/specgate/blob/v0.6.0/docs/ctsc/comparison.md)):

- Both traces are validated first. An invalid trace is reported as a
  validation failure, not a mismatch.
- Scenarios pair by name; a scenario present on one side only is a mismatch.
- Operations and events compare in order, except direct children of a parallel
  region. Faults compare by type and message. Timestamps, ids, durations, and
  a fault's native type do not count.
- With `--registry`, values compare with registry semantics: for example sets
  and maps are unordered and integer ranges are checked. Without it, values
  compare by exact structure.

Pass `--registry` whenever you have one.

## Expected output

Lines are not yet a stable contract
([`../reference/cli.md`](../reference/cli.md#standard-output)); rely on exit
codes.

```text
valid: bundle <capture-dir>
valid: linked candidate.otlp.json
policy: ctsc.strict/0.1.0
reference: <capture-dir>/reference.otlp.json
candidate: candidate.otlp.json
equivalent: true
```

| Exit code | `validate` | `compare` |
| --- | --- | --- |
| 0 | Valid | Equivalent |
| 1 | Invalid | Not equivalent, or an input was invalid |
| 2 | Usage error | Usage error |

## Artifacts

None; both commands only read.

## Failure modes

| Output | Meaning | Next step |
| --- | --- | --- |
| `invalid: <level> <artifact>` plus findings | The artifact breaks a CTSC rule. | Read each `<location>: <message>`; regenerate the artifact. |
| `mismatch scenario["…"]: expected present, actual <missing>` | Scenario names differ. | Replay the reference instead of comparing two independent captures ([`port-a-component.md`](port-a-component.md)). |
| `mismatch <path>: expected <x>, actual <y>` | Behavior differs at `<path>`. | Inspect that operation in both traces. |
| `validation …` lines | An input trace or registry is invalid. | Validate each artifact on its own. |

## Verify

The exit code of `compare` is 0.
