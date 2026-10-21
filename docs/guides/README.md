# SpecGate guides

Task-oriented guides for people and coding agents. Each guide uses the same
sections: Goal, Prerequisites, Steps, Expected output, Artifacts, Failure
modes, Verify, and Source of truth.

Each command is tagged with how it is checked:

- **[smoke]**: run by `just package-smoke` against packaged crates
  (`scripts/package-smoke.ps1`).
- **[it]**: covered by a `specgate-cli` test that runs the real toolchain
  (`rust/crates/specgate-cli/tests/` or `src/capture/tests.rs`).
- **[doctest]**: compiled by `cargo test`.
- **[manual]**: not run by any check.

## Agent quick path

The whole workflow, for a Rust or C# component already annotated and declared
in `specgate.yaml`. Replay needs a second (Rust) implementation, so every
command passes `--impl` (required when a component has several
implementations):

| Step | Command | Success looks like |
| --- | --- | --- |
| 1. Discover (optional) | `specgate discover --component <id> --impl <reference> --registry-id urn:ctsc:registry:<id> --registry-version 0.1.0 --out registry.ctsc.json` | exit 0, `Complete(component=…` |
| 2. Capture | `specgate capture --component <id> --impl <reference> --out capture` | exit 0, `Complete(component=…, scenarios=…` |
| 3. Validate | `specgate validate bundle capture` | exit 0, `valid: bundle …` |
| 4. Replay (Rust candidate) | `specgate replay capture --impl <candidate> --out candidate.otlp.json` | exit 0, `Complete(component=…, plans=…` |
| 5. Compare | `specgate compare capture/reference.otlp.json candidate.otlp.json --registry capture/registry.ctsc.json` | exit 0, `equivalent: true` |

Use exit codes, not stdout text, to decide success: stdout lines are **not yet a
stable contract and may change** ([CLI reference](../reference/cli.md#standard-output)).
On a non-zero exit, look up the message in
[`troubleshoot-capture.md`](troubleshoot-capture.md).

## Which guide do I need?

| I want to… | Guide |
| --- | --- |
| Install the CLI, crates, or NuGet package | [`install.md`](install.md) |
| Annotate a Rust crate | [`instrument-rust-crate.md`](instrument-rust-crate.md) |
| Annotate a C# assembly | [`instrument-csharp-assembly.md`](instrument-csharp-assembly.md) |
| Declare components in `specgate.yaml` | [`write-a-binding.md`](write-a-binding.md) |
| Record reference behavior from my tests | [`capture-a-reference.md`](capture-a-reference.md) |
| Run recorded inputs against another Rust implementation | [`replay-a-candidate.md`](replay-a-candidate.md) |
| Check an artifact or diff two traces | [`validate-and-compare.md`](validate-and-compare.md) |
| Capture `async` Rust or `Task`-returning C# operations | [`async-capture.md`](async-capture.md) |
| Fix a failing command | [`troubleshoot-capture.md`](troubleshoot-capture.md) |
| Check a Rust and a C# implementation agree | [`port-a-component.md`](port-a-component.md) |

## Reference

- [CLI](../reference/cli.md): commands, options, exit codes, output, environment.
- [Binding](../reference/binding.md): every `specgate.yaml` field.
- [Value mapping](../reference/value-mapping.md): Rust and C# types to CTSC.
- [Capture bundle](../reference/bundle.md): what `capture` writes.
- [Limitations](../reference/limitations.md): what is not supported yet.
