# Troubleshoot capture

## Goal

Map a `capture` (or `replay`) error to its cause and fix.

## Prerequisites

- The failing command and its `Error(...)` line.
- [`capture-a-reference.md`](capture-a-reference.md) for what capture runs.

## Steps

1. Find a fragment of the message in the tables below. Messages embed paths,
   names, and counts around these fragments.
2. Apply the fix and rerun the command.
3. If a test was skipped, run it alone with `cargo test <name> -- --exact` or
   `dotnet test --filter …` to see its own output.

A failing test does not fail the capture on its own: it is skipped and listed.
Capture fails when no passing test produced a scenario for the component.

## Expected output

After the fix, `Complete(...)` and exit code 0. [smoke]

## Artifacts

Capture reports no bundle on failure. If the output directory is no longer empty, rerun with `--force`.

## Failure modes

Each fragment below is checked by a test to appear verbatim in the SpecGate
sources, so a search for it finds the code that raises it.

<!-- diagnostics:start -->

### Request and output

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `is not empty; pass --force to overwrite it` | The capture `--out` directory has files. | Use a new directory or `--force`. |
| `already exists; pass --force to overwrite it` | The replay `--out` file exists. | Use a new path or `--force`. |
| `not found in binding` | Unknown `--component`. | Use a component id listed in the message. |
| `not found for component` | Unknown `--impl`. | Use a label listed in the message. |
| `has several implementations; select one by label` | `--impl` omitted for a multi-implementation component. | Pass `--impl`. |
| `tests must declare exactly one of projects or command` | Invalid `tests` block. | Use `tests.projects`. |
| `which is not yet supported (#46)` | `tests.command` in the binding. | Use `tests.projects`. |
| `no operating-system cache directory is available; set SPECGATE_CACHE_DIR` | No `%LOCALAPPDATA%`, `$XDG_CACHE_HOME`, or `$HOME`. | Set `SPECGATE_CACHE_DIR`. |

### Rust capture

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `is not a Rust package` | `sources` has no `Cargo.toml`. | Point `sources` at the package directory. |
| `capture test build failed` | `cargo test --no-run` failed in `sources`. | Fix the build; the message includes Cargo's output. |
| `capture test build produced no libtest binaries` | The package has no test targets. | Add `#[test]` functions. |
| `capture test enumeration failed for` | A test binary could not list its tests. | Run the binary with `--list` to see why. |
| `native capture persistence failed in` | A test could not write its recording. | Read the I/O error that follows the path. |
| `was still open when the capture ended` | An operation never completed, such as a dropped top-level future. | Await it or enclose it in an operation that completes ([`async-capture.md`](async-capture.md)). |
| `native capture cannot instrument an async setup` | An `async` `#[spec_setup]`. | Make the setup synchronous. |
| `ran twice in one capture with different inputs` | A setup ran twice with different inputs in one test. | Construct the receiver once per test. |
| `cannot fold its #[spec_setup] declarations` | Overlapping setups for one operation. | Give each receiver or parameter one setup. |

### C# capture

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `beside the specgate executable` | The C# discovery or capture tools are missing. | Install the full release artifact ([`install.md`](install.md)) or set `SPECGATE_DISCOVERY_PATH` / `SPECGATE_WEAVER_PATH`. |
| `C# capture requires tests.projects` | No `tests.projects` in the binding. | Add the tests project. |
| `targets several frameworks; set csharp.framework in the binding to choose one` | A multi-targeting tests project. | Set `csharp.framework`. |
| `references no supported test framework` | The tests project uses none of xUnit, MSTest, NUnit. | Reference one of them. |
| `references several test frameworks` | More than one is referenced. | Keep one. |
| `C# capture build of` | `dotnet build` of the tests project failed. | Fix the build; the message includes its output. |
| `the capture weaver did not report on the build of` | The weaver did not run on the sources project. | Check that `sources` contains the project the tests reference. |
| `the capture weaver wove no method of` | Discovery found operations the woven build lacks. | Same as above. |
| `printed no test list` | `dotnet test --list-tests` produced no list. | Run it yourself on the tests project. |
| `listed no tests; a capture project must contain tests` | The tests project has no tests. | Add tests. |
| `is listed more than once; scenario names must identify exactly one test` | Two tests share a display name. | Rename one. |
| `a scenario name must match exactly one test` | A filter selected zero or several tests. | Make display names unique. |
| `no result was logged for` | The filter matched nothing or the host crashed. | Run the test alone. |
| `the test host likely crashed before finishing` | The test process ended without a result. | Run the test alone; check its own output. |
| `the test result records a failed outcome` | The test failed; it is skipped. | Fix the test. |
| `instead of Passed, so its recording was not published` | The test ended with another outcome, such as skipped; it is skipped. | Make the test run and pass. |

### C# weave

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `has a SpecOperation annotation without a Spec component.` | `Spec` missing. | Add `Spec = "<component>"`. |
| `is a generic method.` | A generic setup. | Use a nongeneric setup. |
| `requires one concrete TypeArguments entry per generic method parameter` | A generic operation lacks a complete specialization. | Supply concrete `TypeArguments` for every generic method parameter. |
| `is declared on generic type` | Declared on a generic type. | Move it to a non-generic type. |
| `must take parameters by value.` | `ref`, `out`, `in`, or pointer parameters. | Take parameters by value. |
| `Iterator capture is not supported;` | An iterator method. | Return a materialized collection. |
| `Stream capture is not supported;` | An async iterator. | Return a materialized collection. |
| `Async capture supports operations returning Task, Task<T>, ValueTask, or` | `async void`, an async setup, or an `async` method returning another awaitable. | Use a supported return type ([`async-capture.md`](async-capture.md)). |

### Assembly

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `no passing tests captured operations for component` | No passing test called the component. | Read the skipped list that follows. |
| `these tests produced no capture and were skipped` | Introduces the skipped tests and their reasons. | Fix each listed test. |

### Replay

| Message fragment | Cause | Fix |
| --- | --- | --- |
| `replay currently supports only Rust candidates; implementation language is` | Replay onto C#. | Replay onto a Rust implementation. |
| `candidate setup resolution failed` | Setups do not resolve uniquely. | Annotate one matching producer per receiver or injected parameter. |
| `selected setup producers belong to different Cargo test targets` | Required producers do not share a target. | Keep the environment in one Cargo test target. |
| `unsupported async replay scheduling:` | Recorded sibling operations overlap. | Record directly awaited calls; overlapping scheduling is unsupported. |
| `uses unsupported structured type` | An operation or setup input is neither a supported primitive/collection nor an explicitly opted-in property setup. | See [`../reference/limitations.md`](../reference/limitations.md). |

<!-- diagnostics:end -->

## Verify

Rerun the command; it prints `Complete(...)` and exits 0. Then
`specgate validate bundle <capture-dir>`. [smoke]

## Source of truth

- `rust/crates/specgate-cli/src/capture/` and `src/replay/`.
- `rust/crates/specgate-discovery/src/binding.rs`, `src/runner/cache.rs`.
- `rust/crates/specgate-runtime/src/lib.rs`.
- `csharp/SpecGate.Weaver/`.
- [`../reference/limitations.md`](../reference/limitations.md)
