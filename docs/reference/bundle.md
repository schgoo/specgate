# Capture bundle reference

`specgate capture` writes one directory per run. `specgate validate bundle`,
`specgate replay`, and `specgate compare` read it.

```text
<capture-dir>/
  manifest.json        # capture manifest (SpecGate format)
  registry.ctsc.json   # CTSC registry for the component and its dependencies
  reference.otlp.json  # CTSC reference trace (OTLP JSON)
```

`replay` writes its candidate trace into the same directory by default, as
`candidate-<impl>-<capture-dir name>.otlp.json`. Pass `--out` to write it
elsewhere.

## `manifest.json`

Example (from
[`fixture.stateless_add`](../../test/behavior/cases/fixture.stateless_add/expected/manifest.json),
reformatted):

```json
{
  "format": "specgate.capture-manifest",
  "formatVersion": "0.1.0",
  "componentId": "fixture.stateless_add",
  "target": { "name": "expected", "language": "csharp" },
  "tool": { "name": "specgate", "version": "0.6.0" },
  "registry": {
    "path": "registry.ctsc.json",
    "id": "urn:ctsc:registry:fixture.stateless_add",
    "version": "0.1.0",
    "digest": "sha256:…"
  },
  "reference": { "path": "reference.otlp.json", "digest": "sha256:…" },
  "scenarios": {
    "count": 1,
    "names": ["SpecGate.CtscFixtures.Tests.BasicBehaviorTests.StatelessAndMultiOperationFixturesPreserveArithmetic"]
  }
}
```

| Field | Meaning |
| --- | --- |
| `format`, `formatVersion` | Always `specgate.capture-manifest` and `0.1.0`. |
| `componentId` | The captured component. |
| `target.name` | The capture directory's name. |
| `target.language` | `rust` or `csharp`. |
| `tool` | The `specgate` name and version that wrote the bundle. |
| `registry.path`, `reference.path` | Always `registry.ctsc.json` and `reference.otlp.json`. |
| `registry.id`, `registry.version` | `urn:ctsc:registry:<component>` and `0.1.0`. Must match the registry document. |
| `*.digest` | `sha256:` plus 64 lowercase hex digits over the exact file bytes. |
| `scenarios.count`, `scenarios.names` | At least one scenario; names are unique and match the trace. |

The writer emits the manifest as one line of compact JSON. Treat the field set
as versioned by `formatVersion`; other formatting may change.

## `registry.ctsc.json`

A CTSC registry: the operations, inputs, outcomes, and named types the
component publishes. Format: [`../ctsc/registry.md`](../ctsc/registry.md). Type
mapping from native code: [`value-mapping.md`](value-mapping.md).

## `reference.otlp.json`

A CTSC trace in OTLP JSON. Each test is one scenario span; each annotated call is
an operation span under it. Format: [`../ctsc/trace.md`](../ctsc/trace.md).
Scenario names are test names. Rust: the libtest path (`module::test_name`) for
library unit tests, `<test target>::<test path>` for other test targets. C#: the
test framework's display name (for an xUnit fact, the fully qualified method
name).

## Validating a bundle

```text
specgate validate bundle <capture-dir>
```

Checks the manifest fields, both digests, that the manifest matches the
registry and trace, and that the trace is valid against the registry.

## Source of truth

- Writer: `rust/crates/specgate-cli/src/capture/`.
- Validator: `rust/crates/specgate-ctsc/src/validation/bundle.rs`.
- Examples: every `test/behavior/cases/<id>/expected/` directory.
