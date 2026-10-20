# C# capture runs native tests through the CLI

> **Status:** accepted
> **Owner:** SpecGate maintainers (plan approved by the user; `net8.0;net10.0` runtime retarget human-ratified)
> **Date:** 2026-10-17
> **Issue:** [#46](https://github.com/schgoo/specgate/issues/46)

## Context

[`csharp-capture-instrumentation.md`](csharp-capture-instrumentation.md) chose IL
weaving; the weaver and `SpecGate.Runtime` now exist, and
`SpecGate.Runtime` already serializes the exact Rust sidecar bytes. What is
missing is the product path: `specgate capture --impl csharp` must run the
target's native tests, collect one sidecar per test, and publish the same
validated CTSC bundle that Rust capture publishes. Today `capture` rejects every
non-Rust implementation in `require_capturable`.

## Decision

### Runtime activation

1. **The runtime owns activation.** `SpecGate.Runtime` carries a
   `[ModuleInitializer]` that reads `SPECGATE_NATIVE_CAPTURE`. The weaver injects
   no initializer IL, so woven assemblies stay a pure function of their
   annotations.
2. **Wire format** is exactly the Rust `EnvConfig`:
   `{"capture":{"scenario_name","trace_id","run_span_id","scenario_span_id","operation_span_ids","start_time_unix_nano","clock_step_unix_nano"},"sidecar_path"}`.
   An absent or empty variable is a silent no-op. A present but invalid value
   fails loudly with `invalid SPECGATE_NATIVE_CAPTURE configuration`.
3. **The environment session records every component**, as Rust does; component
   selection is the CLI's job. The existing single-component
   `BeginSession(component, config)` stays for the runtime's own tests. Starting
   a session twice is idempotent.
4. **Persistence follows Rust.** The sidecar is rewritten atomically (temp file
   in the same directory, flush, replace) after inputs are recorded and after
   every operation closes. A persistence failure throws inside the instrumented
   operation so the test fails. At `AppDomain.ProcessExit` any operation still
   open is closed as an `incomplete_capture` fault and persisted without
   throwing; a process with nothing open writes nothing. There is no separate
   `.failure` file: the thrown exception is the failure signal.
5. Output is UTF-8 without BOM, LF, invariant culture.
6. `SpecGate.Runtime` multi-targets `net8.0;net10.0`. `ModuleInitializer` and
   `ProcessExit` handling need a modern runtime and every maintained .NET
   baseline is 8+. `SpecGate.Annotations` and `SpecGate.Weaver` stay
   `netstandard2.0`: annotations are referenced by arbitrary targets and the
   weaver runs inside MSBuild. Woven capture therefore needs a .NET 8+ target.

### Weaving under the CLI

7. **A CLI-injected targets file** (`SpecGateCapture.targets`, shipped beside the
   weaver) is supplied with `-p:CustomAfterMicrosoftCommonTargets`. It applies
   only when `$(MSBuildProjectFullPath)` equals the CLI-passed
   `SpecGateWeaveProject`, so the sources project alone is woven. It adds the
   runtime reference so a copy of `SpecGate.Runtime.dll` lands beside the woven
   assembly, sets `SpecGateWeaverTaskAssembly`, and keeps `TaskHostFactory`.
   Target projects need no edits.
8. The targets file writes a machine-readable woven-method-count report. The CLI
   uses it as a canary: a woven count of zero while discovery found operations
   is an error, never a silent pass-through.
9. Weaving stays idempotent and verifiable (`IsWoven`).
10. Shipping: weaver, runtime and targets live beside the `specgate` executable
    and `SPECGATE_WEAVER_PATH` overrides the directory, mirroring
    `SPECGATE_DISCOVERY_PATH`.

### CLI orchestration

11. **Language dispatch is a closed `match` on `Language`** in `execute_with`; no
    trait object. Language-neutral code (`ExecutedTests`, `FailedTest`,
    `reject_failures`, unfinalized handling) stays in `capture/execution.rs`; a
    new `capture/sidecar.rs` holds per-test env configuration, sidecar parsing,
    foreign/duplicate checks and unfinalized detection; `capture/rust.rs` holds
    cargo build/enumerate/libtest and `capture/csharp.rs` the .NET runner.
    `require_capturable` keeps only the `tests.command` rejection
    (`not yet supported (#46)`); `tests.command` capture stays out of scope.
12. **C# runner.** One `dotnet build` per batch of each `tests.projects` entry:
    `dotnet build <tests project> -c Debug --artifacts-path <scratch>
    -p:SpecGateWeaveProject=<sources project>
    -p:CustomAfterMicrosoftCommonTargets=<targets> -nodeReuse:false
    -p:UseSharedCompilation=false`, plus `--framework` when `csharp.framework`
    is set (several target frameworks without it is an error). Tests are listed
    with `dotnet test --no-build --list-tests` and run one per process with an
    exact filter and `SPECGATE_NATIVE_CAPTURE` set. Children run with
    `DOTNET_NOLOGO=1`, `DOTNET_CLI_TELEMETRY_OPTOUT=1`,
    `DOTNET_CLI_UI_LANGUAGE=en`; output is CR-stripped.
13. **Sources and tests are different paths.** `sources` (discovery and the weave
    target) is the project under capture; `tests.projects` is what is built,
    listed and run. The same environment contract applies to every project;
    scenario names are prefixed with the project file stem only when more than
    one project is declared, duplicate names are rejected, and ordering is
    ordinal on bytes.
14. **Theory and data-driven cases are separate scenarios.** The scenario name is
    the listed name. The runner filters on that name with the VSTest filter
    specials (`\ ( ) & | = ! ~`) backslash-escaped, `%` as `%25`, `,` as `%2C`
    and `;` as `%3B`, and requires a TRX log showing exactly one executed test.
    A name that matches zero or several tests fails the whole batch.
    **Test frameworks: xUnit, MSTest and NUnit.** The adapter is detected per
    tests project from the evaluated `PackageReference` items
    (`dotnet msbuild -getItem:PackageReference`, under `-p:TargetFramework`
    when `csharp.framework` is set), so the binding gains no field. `xunit`,
    `xunit.core`, `xunit.v3*` and `xunit.runner.visualstudio` select xUnit;
    `MSTest`, `MSTest.TestFramework` and `MSTest.TestAdapter` select MSTest;
    `NUnit` and `NUnit3TestAdapter` select NUnit (package ids compare
    case-insensitively). No match, or packages of several frameworks, fails
    closed before anything is built. The filter grammar and its escaping are
    VSTest's and identical for all three; only the property differs: xUnit
    matches the listed name with `DisplayName`, while the MSTest and NUnit
    adapters implement no `DisplayName` and list `Name` (the method name plus
    its arguments, without the class). Two classes that share a method name
    therefore list the same name and are rejected as a duplicate scenario
    rather than run together. Other frameworks (Expecto, TUnit, Microsoft
    Testing Platform runners) are not supported.
15. **Sidecar failure modes** (all covered by fake-driven tests):

    | Situation | Result |
    | --- | --- |
    | sidecar missing, test passed | skipped, unless the woven count is zero while discovery found operations, which fails |
    | sidecar missing, test failed | `FailedTest::TestFailed` |
    | duplicate, malformed or foreign sidecar (identity differs from the issued config, or path outside scratch) | explicit whole-batch error |
    | unfinalized recording | `FailedTest::Unfinalized` |

    The sidecar is deleted before every run. Fixed ids, start time 0, clock step
    1, CRLF stripped and no machine paths in artifacts keep bundles
    byte-identical across Windows and Linux.
16. Async setups remain rejected by `validate_setups`.

### Non-goals

C# rows in the golden matrix and Rust-versus-C# strict comparison belong to
[#47](https://github.com/schgoo/specgate/issues/47). No matrix, golden, CLI flag,
CTSC document shape or manifest field changes here.

## Spike findings (this machine: Windows, SDK 10.0.402, xunit.v3 3.2.2, VSTest)

A throwaway xunit.v3 project with a `[ModuleInitializer]` that logs when
`SPECGATE_NATIVE_CAPTURE` is set and registers a `ProcessExit` handler showed:

- `dotnet test --no-build --filter ...` propagates the environment variable to
  the process that runs the tests, and `ProcessExit` fires there. Several helper
  processes also load the module and fire `ProcessExit`, which is why a process
  with nothing recorded must write nothing.
- `--list-tests` prints one `    <display name>` line per test below
  `The following Tests are available:`; Theory cases appear with their data
  (`Spike.SpikeTests.Theorized(n: 2, s: "c(d)|e&f")`). The whole output after
  the header is scanned, so warnings or blank lines between entries are ignored
  rather than truncating the list; a missing header or an empty list stops the
  batch.
- A filter of `DisplayName=<escaped display name>` selects exactly that case,
  provided `,` is written `%2C`; a raw comma is split by MSBuild property parsing
  (`MSB4177`). The TRX contains exactly one `UnitTestResult`.
- A zero-match filter exits `0` and prints `No test matches the given testcase
  filter`; it is detected from the TRX having no result, never from the exit
  code. A missing or truncated TRX (crashed test host, bad filter) stops the
  whole batch with an error naming the test, in strict and non-strict capture
  alike; only a complete TRX recording a failed result is an ordinary failed
  test. The recorded outcome gates publication: only `Passed` keeps a
  finalized sidecar's scenario. `NotExecuted`, `Skipped` and other non-passed
  outcomes are not captured, and strict capture fails the batch on them
  (`FailureReason::NotPassed`), as it does for any failed test.
- Capture and discovery pick the sources project by the same rule: the
  ordinally first `.csproj` in the directory, independent of listing order.
- One `dotnet test --no-build` costs about 3 s on this machine, so capturing the
  36-test fixture project costs about two minutes per run.

## MSTest and NUnit findings (Windows, SDK 10.0.402, MSTest 3.10.4, NUnit 3.13.3 with NUnit3TestAdapter 4.6.0, VSTest)

Small fixture projects (`tests-mstest`, `tests-nunit` beside the xUnit one in
`rust/crates/specgate-cli/tests/fixtures/csharp-capture`) restore from the local
package cache and build in seconds, and the integration test
`csharp_capture_selects_mstest_and_nunit_cases` captures both end to end
(about 40 s for the two together, run in parallel, so it adds about that much
to `just check`; `package-smoke` and `release.yml` still use the xUnit fixture
and are unchanged):

- `--list-tests` prints `AddsEveryCase (1,2,3)` for an MSTest `DataRow` (with a
  space) and `AddsEveryCase(1,2,3)` for an NUnit `TestCase`: one line per case,
  without the class.
- `Name=AddsEveryCase \(1%2C2%2C3\)` selects exactly that MSTest row and
  `Name=AddsEveryCase\(1%2C2%2C3\)` exactly that NUnit case, one TRX result each.
  The method name alone selects nothing for MSTest and both cases for NUnit,
  and an unescaped `(` is a VSTest filter format error, so the escaping above is
  required for both.
- `FullyQualifiedName=` with the method name selects nothing for MSTest, so
  `Name` is the property to use.

Verification gap: only these three frameworks at these versions were run, on
Windows. Parameterized names that contain filter specials were verified live
for xUnit only; MSTest and NUnit share the escaping code and have fake-driven
tests, but no live case with a literal `;`, `&` or `%`. MSTest's data-row
unfolding is version dependent (older adapters list one test for all rows, which
the one-result TRX check then rejects). Nothing here was run on Linux.

## One build per batch

The sources project is compiled once per capture batch: the woven build the tests
run against. Reflection reads that woven assembly
(`specgate_discovery::discover_built`) instead of building the sources again, so
an A→B capture went from three builds (initial discovery, woven build, isolated
nested-schema discovery) to one. The discovery tool, run with no component
names, publishes every inventoried component; the CLI keeps those schemas on
`ExecutedTests` and encodes each bundle from them, so nested foreign components
are linked-validated from the same metadata. Consequence: an unknown component
now fails after the build rather than before. The tool's command line is
internal; no CLI flag, binding field or CTSC shape changed. A fake-driven test
asserts exactly one `build` request for an A→B batch.

## Consequences

- Capturing C# needs a .NET 8+ target and one `dotnet` process per test.
- Per-test process cost dominates; it is measured and reported rather than
  redesigned here.

## Implementation outcome

Recorded after the slices landed, where the build differed from the plan:

- **Weaver directory.** One flat directory holds the weaver build output (Cecil
  included), `SpecGate.Runtime` built with `--framework net8.0`, Annotations and
  both targets files. `just weaver-tool` builds it into `artifacts/weaver`.
- **Weave report.** The targets file writes `{"woven_method_count":N}` to
  `SpecGateWeaveReportPath`. The canary is therefore batch-level: it replaces
  the per-test "missing and passed" rule, and a count of zero with discovered
  operations fails the batch.
- **Foreign check.** Only identity is verified (scenario name, trace id, span
  ids). A path check is moot because the CLI issues the sidecar path.
- **Duplicate.** Covered by the TRX count (anything but one result is an error)
  plus rejection of duplicate listed names.
- **Persistence.** Rust-equivalent semantics: the sidecar is rewritten after
  inputs are recorded and after every operation completion, a superset of
  "every top-level operation". Outstanding operations are projected as
  `incomplete_capture` faults in each snapshot; `ProcessExit` is a final
  non-throwing flush. A persistence failure throws inside the operation
  (`CaptureRejection.PersistenceFailed`, message prefixed with the Rust failure
  marker). The failure is sticky: the session is poisoned, so a test that
  catches the exception cannot get a later write to publish the earlier
  operation as successful. Every later write throws; at `ProcessExit` the
  runtime prints the marker to stderr and makes a best-effort sidecar write with
  every operation projected as `incomplete_capture`. The CLI rejects the whole
  batch when the marker appears in a test's output, and treats such a sidecar as
  unfinalized. Limit: if the sidecar stays unwritable and the stderr marker is
  not forwarded by `dotnet test`, nothing further can be reported (the stderr
  path is not yet verified live).
- **Nested components.** C# discovery reports only the requested component, but
  the filter keeps nested calls into other components. `capture/facade.rs`
  discovers every retained foreign component and `encode_bundle` uses those
  schemas (as Rust does with its whole-crate registry); the encoded trace and
  registry are linked-validated before success for every language.
- **Framework detection.** Multi-targeting is read from evaluated MSBuild
  properties (`dotnet msbuild -getProperty:TargetFramework
  -getProperty:TargetFrameworks`) through the mockable execution boundary, so
  settings inherited from `Directory.Build.props` count. Several frameworks
  without `csharp.framework` is an error before anything is built.
- **Filter escaping.** `dotnet` passes `--filter` through MSBuild, which decodes
  `%XX` and splits on `,`/`;` and rejects `"`; `exact_filter` therefore also
  escapes `%` as `%25` (first) and `"` as `%22`. Linux behavior is unverified.
- **Environment hygiene.** Every C# tool request removes an inherited
  `SPECGATE_NATIVE_CAPTURE`; only the per-test request sets it.
- **Operation↔component is 1:1 (ratified, option F).** Copilot finding 2 showed
  that a method declaring several components could only record under the first.
  Rather than pick an attribution rule, an operation now belongs to exactly one
  component. `SpecOperationAttribute` no longer allows multiple use (the
  compiler rejects repeats, CS0579), the weaver throws `WeaveException` naming
  the method and components for an assembly that still carries several
  (`RejectsMethodDeclaringSeveralComponents`, built with Cecil), and
  `specgate-discovery` exits 1 with the same message
  (`AMethodDeclaringSeveralComponentsIsRejected`). The first-declared-wins loop,
  the `string[]` `EnterOperation` overload and their tests were deleted. The
  fixture `StatelessAdd.Add` was split so `fixture.multi_case` has its own
  `MultiCase.Add`; the matrix row now lists `MultiCase.cs`. Setups are
  unchanged.
- **I/O seam.** The C# runtime uses an interface (`ISidecarFileSystem`) with a
  fake; the Rust CLI uses the existing struct-with-inner-enum boundary.
- **Discovery per component.** C# discovery reports only the requested
  component, so one execution (which records every component) feeds one
  `write_bundles` call per separately discovered component.
- **Cost.** Measured per-test cost is about 3 s (one `dotnet test --no-build`),
  and a batch builds once. Running the 36-test `CtscFixtures.Tests` project
  costs about 130 s per batch, so the integration test no longer uses it: it
  captures a dedicated small fixture pair
  (`rust/crates/specgate-cli/tests/fixtures/csharp-capture`: a sources project, a
  four-test passing project with a Theory of two cases, and a one-test failing
  project) and runs the two woven captures, the unwoven control and the failing
  fixture concurrently. It finishes in about 40 s, down from about 235 s. The
  packaged-install smoke and the release workflow capture the same passing
  fixture, so they need no failure tolerance.
- **Fixture conflict (tracked in #47).** `SetupFoldingTests.*` (8 tests in
  `CtscFixtures.Tests`) call `CaptureScenario.Begin`, which starts a runtime
  session. Under CLI capture the environment session is already active, so they
  fail with "a capture session is already active". Non-strict capture tolerates
  the failures; strict capture and the golden harness would not. The binding at
  `test/bindings/specgate.yaml` stays pointed at that project for
  [#47](https://github.com/schgoo/specgate/issues/47), which owns the decision
  (retire the in-process `Begin` for native capture, or make it join the
  environment session). Nothing in #46 exercises it.
- **Unverified.** `;` as `%3B` in filters, fully-qualified-name filtering, and the
  textual multi-TFM check, which misses values set only in props files.
  **Linux is untested:** every run and measurement above was on Windows. Path
  handling, CRLF stripping, the executable-directory lookup and process
  environment propagation are written to be platform neutral, and CI's Linux and
  macOS release jobs exercise the packaged capture, but no local Linux evidence
  exists.
