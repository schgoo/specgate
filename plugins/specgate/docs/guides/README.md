# SpecGate guides

Task-oriented guides for people and coding agents. Each guide uses the same
sections: Goal, Prerequisites, Steps, Expected output, Artifacts, Failure
modes, and Verify.

## Agent quick path

Start with step 0 when you are adding SpecGate to an existing API. Skip it only
when the requested component is already annotated, bound, and tested.

### Step 0: choose the boundary before instrumenting

Before you annotate anything, write down:

| Item | What to record |
| --- | --- |
| Reference API | The exact types and methods the user asked you to compare. |
| Candidate mapping | The Rust function or method that implements each reference operation, with the same component id, operation names, and input names. |
| Existing tests | The tests that already call the reference API. Each passing test that calls one of the component's operations directly becomes a scenario. |
| Semantic inputs | Where each operation input comes from: a parameter, or a setup construction input. |
| Setup producers | Which annotated setup builds each receiver or injected parameter, in each language. |
| Observable results | The return values, declared errors, and nested annotated operations that comparison will check. |

Then follow these defaults:

- **Annotate the existing boundary.** Put `[SpecOperation]` or
  `#[spec_operation]` on the requested API itself, not on a new wrapper around
  it.
- **Reuse the existing tests.** Capture runs them unchanged. Add a test only for
  behavior they do not cover, and do not replace the suite with new scenario
  tests.
- **Record semantic values.** Each recorded input must be the value the
  operation actually uses, such as `initial = 9`. A label such as
  `"large-polygon"`, which each implementation maps to its own hard-coded
  fixture, is not a semantic input: comparison never sees the data, so the two
  fixtures can differ undetected.
- **Stop and ask before substituting.** If a limitation blocks the requested
  API, report the exact message and its
  [limitations](../reference/limitations.md) row. Get approval before you
  instrument a different API or replace the requested boundary with a scenario
  wrapper.

Adapters are fine when they keep the requested boundary and its semantic
values:

- **Allowed:** setup producers and construction helpers that build a receiver or
  injected parameter from recorded semantic inputs, and canonical observation
  adapters that expose one shared result contract
  ([typed async replay](https://github.com/schgoo/specgate/blob/v0.6.0/docs/decisions/typed-async-client-replay.md#canonical-adapters-and-structured-inputs)).
- **Needs approval:** a scenario API that replaces the requested boundary, such
  as `string RunScenario(string fixtureName)`.

Replay accepts primitives and recursively supported lists, sets, string-keyed
maps, and optionals as inputs. A class or struct parameter can be deconstructed
into its fields: opt the type in as a property setup (`#[spec_event(setup)]` or
`[SpecEvent(Setup = true)]`), and each selected field becomes a flat semantic
input. A field whose type is itself an opted-in record is deconstructed
recursively. Every other field must be a primitive, collection, or optional,
and records inside collections or optionals are not deconstructed. Leaf names
are flat, so rename colliding fields. Otherwise, give the parameter an explicit
setup producer. A record input with neither, or a field holding a record that
is not opted in, fails with `structured-value`. Report that limitation; do not encode structured
inputs as fixture names.

[Port a component](port-a-component.md#worked-example-setup-backed-method)
shows the intended shape: an existing C# test calls the real setup and method,
and replay rebuilds the Rust receiver from the recorded `initial` input.

#### Examples to follow

These executable fixtures show instrumentation patterns. Apply a pattern to the
target's existing APIs and tests. Do not reproduce a fixture as a new
fixture-name scenario wrapper. Reuse existing tests first, and add cases only
where coverage is missing.

| Example | What it teaches |
| --- | --- |
| [`fixture.stateless_add`](https://github.com/schgoo/specgate/tree/v0.6.0/test/behavior/cases/fixture.stateless_add) | Annotate the implementation. Ordinary native tests call it, and capture records the actual arguments and result. |
| [`fixture.setup`](https://github.com/schgoo/specgate/tree/v0.6.0/test/behavior/cases/fixture.setup) | Setup construction inputs become semantic operation inputs. Each language keeps its own object behind its setup producer. |
| [`fixture.property_setup`](https://github.com/schgoo/specgate/tree/v0.6.0/test/behavior/cases/fixture.property_setup) | Parameter objects opted in as property setups: their fields, including collections, optionals, and nested opted-in records (also as setup-producer arguments), become flat semantic inputs, and replay rebuilds the Rust value. Also a C# constructor setup. |
| [`typed-integration.yaml`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/tests/fixtures/typed-integration.yaml) | Generic async client methods, typed results, and test-local setups that rebuild the environment from recorded inputs. |

Files that show the pattern:

- **`fixture.stateless_add`:**
  - Operation `add`: [`StatelessAdd.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.stateless_add/csharp/StatelessAdd.cs)
    and [`stateless_add.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.stateless_add/rust/stateless_add.rs).
  - Tests: `StatelessAndMultiOperationFixturesPreserveArithmetic` in
    [`BasicBehaviorTests.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.stateless_add/csharp/tests/BasicBehaviorTests.cs),
    and `add_two_and_three` in `stateless_add.rs`.
  - No setup producer.
  - Registry: [`registry.ctsc.json`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.stateless_add/expected/registry.ctsc.json).
- **`fixture.setup`:**
  - Operations `increment` and `observe_count`, with setups `Counter.Make` and
    `make_counter`: [`Setup.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.setup/csharp/Setup.cs)
    and [`setup.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.setup/rust/setup.rs).
  - Tests: `increments_setup_counter` in `setup.rs`. In C#,
    `FocusedRichAndSetupFixturesRetainBehavior` in
    [`fixture.rich`'s `RegistryBehaviorTests.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.rich/csharp/tests/RegistryBehaviorTests.cs)
    calls this case's
    [`RegistryBehaviorTests.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.setup/csharp/tests/RegistryBehaviorTests.cs)
    step.
  - Registry: [`registry.ctsc.json`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.setup/expected/registry.ctsc.json).
    Its `increment` operation lists the folded `initial` input, not a `Counter`.
- **`fixture.property_setup`:**
  - Property-setup types `Query`, `RenamedSubset` (a field subset with a
    renamed `limit` field), and `Empty`, plus the C# `Constructed` constructor
    setup: [`PropertySetup.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.property_setup/csharp/PropertySetup.cs)
    and [`property_setup.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.property_setup/rust/property_setup.rs).
  - Tests: `ConstructorAndPropertyInputsAreBehavioral` in
    [`PropertySetupTests.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.property_setup/csharp/tests/PropertySetupTests.cs),
    and `constructor_and_property_inputs_are_behavioral` in `property_setup.rs`.
  - Registry: [`registry.ctsc.json`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/cases/fixture.property_setup/expected/registry.ctsc.json).
    The golden matrix replays the C# reference against Rust.
- **`typed-integration.yaml`** (component `fixture.typed_async`):
  - C# operation `find_items` (`FindByTypeNameAsync<T>`) and setups
    `MakeClient` and `MakeOffset` (`Fills = "offset"`):
    [`Client.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/tests/fixtures/typed-csharp/sources/Client.cs).
    Test: [`ClientTests.cs`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/tests/fixtures/typed-csharp/tests/ClientTests.cs).
  - Rust operation `find_by_type_name::<Item>`:
    [`lib.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/tests/fixtures/typed-async/src/lib.rs).
    Setups `make_client` (which starts a local HTTP server) and `make_offset`,
    and test `typed_async_client`, are test-only code in
    [`setup.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/tests/fixtures/typed-async/src/setup.rs).
  - Registry: not checked in. The CLI test `typed_csharp_to_rust_setup_replay`
    in [`replay/tests.rs`](https://github.com/schgoo/specgate/blob/v0.6.0/rust/crates/specgate-cli/src/replay/tests.rs)
    captures the C# reference into a temporary directory and replays it
    against Rust.

Fixture plumbing, not a pattern to copy:

- **Aggregate builds.** [`test/behavior/specgate.yaml`](https://github.com/schgoo/specgate/blob/v0.6.0/test/behavior/specgate.yaml)
  binds every behavior case to two shared projects under `test/behavior/projects/`
  through YAML anchors. Your binding names your own sources and test projects.
- **Golden bookkeeping.** `test/behavior/matrix.json` rows, C# `…Step1` partial
  helpers, and the case `README.md` files track shared scenarios for the
  golden harness.
- **Crate sharing.** `typed-integration/Cargo.toml` and
  `contracts/client.rs` reuse the `typed-async` sources through `path`
  settings, so that one source tree serves two bindings.
- **Generated goldens.** Each case's `expected/` directory is written by
  `just ctsc-goldens-update`. Never copy or hand-edit it. Your bundles come
  from `specgate capture`.

When you finish, report:

- the instrumented boundary in each language, and whether it differs from the
  requested API;
- which tests were reused and which you added;
- what the comparison establishes: on the recorded scenarios' inputs, the
  candidate produced the same results, errors, and nested annotated operations
  under `ctsc.strict/0.1.0`;
- what it does not establish: behavior no test exercised, unannotated
  internals, and anything excluded by a limitation.

### Steps 1–5

The workflow for a Rust or C# component that is annotated and declared in
`specgate.yaml`. Replay needs a second (Rust) implementation, so every
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
