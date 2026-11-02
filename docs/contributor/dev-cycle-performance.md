# Dev-cycle performance

Measurements for [#87](https://github.com/schgoo/specgate/issues/87), collected
on 2026-10-19 and 2026-10-20. The optimizations target compilation reuse, not a
reduced gate.
`just check` runs every gate recipe, grouped into three
[CI lanes](#ci-lanes).

## Golden regeneration

The harness already batches discovery and capture by fixture source root,
reflects C# components in one batch, and shares a replay runner target directory
within each candidate batch. It does not compile a fixture separately for every
matrix row.

However, Rust discovery, capture, negative builds, candidate discovery, and
replay use separate target directories. Each fresh target directory recompiles
common dependencies. Warming the product workspace does not eliminate this
work.

The golden wrapper now sets Cargo 1.96's
[`CARGO_BUILD_BUILD_DIR`](https://doc.rust-lang.org/cargo/reference/config.html#buildbuild-dir)
to `rust/target/ctsc-goldens-build-cache/v2/<slot>/build` **only while executing
the golden harness**.
This shares Cargo intermediate artifacts across those builds without changing
their final target directories. Cargo still resolves the exact dependencies
and fingerprints their source, features, and compiler configuration.

Generated test-target metadata and replay builds set `CARGO_INCREMENTAL=0`.
Their entry and dispatch environment changes procedural-macro output; reusing
that incremental expansion state caused duplicate registrations and Windows
capture-process crashes when switching back to ordinary tests. Dependency
artifacts still share the intermediate directory, and caller compiler flags
are preserved. Ordinary capture builds retain their incremental configuration.

The change applies to both Check and Update. It does not cache registries,
captures, replay results, comparisons, or validation results. Each invocation
holds an exclusive OS-backed file lease on its intermediate cache slot until
the harness exits, including while its generated binaries execute. A concurrent
invocation takes a different slot instead of racing to rebuild running binaries.
Subsequent invocations reuse a successfully completed idle slot. Generated
sources, final outputs, and sidecars retain their existing invocation-local
isolation and cleanup.

Before launching a harness, the wrapper writes and flushes an incomplete marker
into its lease file. Only confirmation of exactly one successful golden test
clears that marker. File handles release on errors or process termination, but
an incomplete slot stays quarantined: surviving harness/Cargo/test children
must never race a new invocation for those intermediates. Failed or interrupted
slots are never automatically recycled. The `v2` layout also isolates these
slots from the earlier lease-only implementation.

Only sharing-violation/lock-busy errors or incomplete slots select another slot;
permission and other I/O failures abort. Successful serial invocations reuse
the same slot; concurrency and failed runs can allocate additional slots.
Like a regular Cargo build directory, each slot can grow as sources and
toolchains change; there is no automatic eviction. Remove
`rust/target/ctsc-goldens-build-cache` only after all golden wrappers, harnesses,
and their Cargo/test children have exited to reclaim space or measure an
empty-cache run.

### Initial optimization: invocation-local sharing

These are serial executions of the same compiled, ignored
`goldens::tests::golden_matrix` test, with the same sources and short temporary
scratch layout. The only difference is whether `CARGO_BUILD_BUILD_DIR` is
unset or points to the invocation's `build` directory. All four runs passed:
60 component rows, 11 negative rows, 6 replacement rows, native validation,
parity, coverage, replay, and byte comparison against checked-in goldens.

| Run | Separate intermediates | Shared intermediates | Saved |
|---|---:|---:|---:|
| First measured | 378.002 s | 139.712 s | 238.290 s (63.0%) |
| Repeated, warm host caches | 376.869 s | 135.481 s | 241.388 s (64.1%) |

Mean regeneration time fell from **377.436 s to 137.597 s**, approximately
**2.74 times faster**. Both configurations start each run with empty scratch:
"warm" means the workspace/tool/download/host caches, not a retained golden
build directory. These are individual samples, not a statistical benchmark or
a claim about Linux savings.

With that initial implementation, `just ctsc-goldens-update` also passed from
the original long-path worktree in **118.231 s**, regenerating all
**232 artifacts with no byte changes**. Its complete `just check` passed in
**648.849 s**; its golden-check stage took **90.275 s**. These recipe timings are additional
verification, not substitutes for the controlled paired comparison above.

### Follow-up: reuse compilation between runs

A second serial experiment retained one intermediate directory across three
executions of the same compiled harness. Each execution still had fresh,
separate invocation scratch and final outputs, and passed the full matrix and
byte comparison:

| Intermediate cache | Golden harness |
|---|---:|
| Initially empty | 109.242 s |
| Retained, second invocation | 62.503 s |
| Retained, third invocation | 61.294 s |

The retained-cache mean is **61.899 s**, **43.3% less time** than the empty-cache
run of this experiment. Do not attribute the difference between 109.242 s and
the earlier 137.597 s mean to retention: both began with empty intermediates
and were collected at different times on the shared host.

The normal `just ctsc-goldens-update` recipe, including its discovery-tool and
regression prerequisites, took **114.527 s** with empty intermediates and
**70.787 s** on the next invocation: **38.2% less time**. Both regenerated all
**232 artifacts byte-identically**. The worktree-local intermediate cache used
957.5 MiB after the first run and 962.6 MiB after the second; these are observed
sizes, not an upper bound.

The final complete Windows `just check` passed in **708.753 s**, with the
golden harness itself taking **57.88 s** (about 60 s for the golden-check
recipe at the log's one-second timestamp resolution). The full gate was
slower than the earlier 648.849 s sample because unrelated stages varied;
this is not evidence of an overall gate speedup. The serial regeneration
comparisons above isolate the intended improvement.

Regressions exercise two simultaneous in-process leases and terminate a real
wrapper while its native child survives and continues writing to its cache
slot. A second invocation must not reuse that slot, even though the wrapper's
OS lease has released. They also verify propagation of non-contention I/O
errors, successful slot reuse, and quarantine on failed or unproven test runs.
A real Cargo fixture compiles
different generated runners into separate final target directories while
sharing intermediates: the unchanged dependency must be fresh on the second
build, an edited dependency must rebuild, and each executable must produce
the new expected value. Wrapper regressions also cover Check, Update, failure
propagation, environment restoration, and lease release.

Local environment: Windows 10.0.26300, Intel Xeon Platinum 8370C, 8 cores /
16 logical processors, Cargo/Rust 1.96.0, .NET SDK 10.0.402, PowerShell 7.6.7,
just 1.42.0. Measurements ran on a shared host; absolute times are not portable.

### CI cache retention

The existing `Swatinem/rust-cache` action recursively prunes workspace target
directories before saving. Its dependency-only cleanup is not suitable for the
golden cache's custom build layout or lease markers. CI therefore sets
`SPECGATE_CTSC_GOLDENS_BUILD_CACHE` to
`artifacts/ctsc-goldens-build-cache/v2`, outside those target directories, and
uses a separate `actions/cache` entry. Local recipes retain their default path.
The override is resolved relative to the repository root, not the caller's
working directory; absolute paths also work.

Keys include OS, architecture, the installed compiler version hash, and hashes
of Cargo manifests, lockfiles, toolchain files, and Cargo configuration,
excluding generated target/artifact directories. Entries are immutable: a
successful job seeds a missing key, and
subsequent exact hits do not upload another copy. Cargo still fingerprints
source edits, features, and compiler options; unchanged cache keys do not
authorize reuse of stale compiled code. No captures, replay outputs, golden
artifacts, or validation results are stored. Failed jobs do not publish a cache,
and the existing lease/quarantine protocol is unchanged. Normal GitHub cache
branch scoping applies; no extra workflow permissions are granted. Golden
generation and all validators still run.

In [run 39419173116](https://github.com/schgoo/specgate/actions/runs/39419173116),
before explicit retention, the golden recipe took approximately **47 s on
Linux** and **90 s on Windows**, including harness preparation. The native
harness took **41.57 s** and **78.92 s**, respectively. Smoke took approximately
**125 s** and **244 s**. Retention can reduce only part of the golden stage;
additional archive transfer and extraction must be subtracted from any saved
compilation. Local retained-cache savings are not evidence of a net CI gain.
Compare cold and restored-cache attempts of the same CI revision, including the
cache restore/save steps, before attributing an improvement to retention.

## Gate stage breakdown

Baseline sources: commit `352d8acdac197b5211b0a907d82e2af61d98ca00` and
[CI run 39042012039](https://github.com/schgoo/specgate/actions/runs/39042012039).
CI durations are differences between logged command timestamps, ending at the
next stage or the end of the gate. CI restored its configured Rust caches; it
is not a cold-cache benchmark. Local durations use a monotonic stopwatch at
`just --verbose` recipe boundaries and include shell startup.

| Stage | Linux CI baseline | Windows CI baseline | Windows local first build | Windows local warm, first optimization |
|---|---:|---:|---:|---:|
| `build` | 16.07 s | 58.02 s | 106.91 s | 4.14 s |
| `discovery-tool` | 2.26 s | 6.71 s | 5.01 s | 2.26 s |
| `test` | 206.04 s | 556.94 s | 405.71 s | 322.90 s |
| `clippy` | 6.37 s | 15.94 s | 41.17 s | 4.31 s |
| `format-check` | 0.32 s | 2.03 s | 4.18 s | 3.53 s |
| `deny` | 0.16 s | 0.85 s | 1.99 s | 1.68 s |
| `readme-check` | 0.29 s | 3.07 s | 7.91 s | 6.85 s |
| `ctsc-validate` | 5.07 s | 14.04 s | 27.35 s | 1.59 s |
| `ctsc-smoke` | 84.57 s | 266.43 s | 238.76 s | 180.82 s |
| `ctsc-goldens-regressions` | 0.28 s | 0.86 s | 1.12 s | 1.06 s |
| `ctsc-goldens-check` | 85.32 s | 241.05 s | failed after 18.43 s | 90.28 s |
| `dotnet-build` | 5.88 s | 20.04 s | not reached | 7.19 s |
| `dotnet-test` | 5.43 s | 12.89 s | not reached | 5.17 s |
| `format-check-cs` | 10.74 s | 22.12 s | not reached | 16.98 s |
| **Total** | **428.79 s** | **1221.01 s** | **858.62 s, failed** | **648.85 s, passed** |

The local worktree initially had no Rust target directories or tooling output;
existing tool/download caches were not deleted. Its baseline gate hit MSVC
LNK1104 when a nested golden dependency build-script output exceeded the
Windows path limit (275 characters in this worktree). The corresponding shared
intermediate path is 213 characters. The paired harness measurements above use
short temporary scratch for **both** configurations so this pre-existing
failure does not distort their comparison.

The warm local run used the initial, invocation-local wrapper. Its full-gate
total is not an isolated measure of the optimization: workspace caches and unrelated
test/smoke timings also changed. Use the paired golden measurements to
attribute savings.

Rust tests, end-to-end smoke, and golden regeneration dominate the baseline.
The discovery-tool prerequisite executes once in the full gate, not once per
dependent recipe. Packaging smoke is a separate Linux CI step, not part of
`just check`; it is unchanged.

## Capture/replay smoke

The smoke wrapper previously used `cargo run` for every CLI invocation. Its
two captures and two replays also compiled dependencies into separate target
directories, repeating compilation even when the product workspace was warm.

The wrapper now builds the CLI once and selects its executable from Cargo's
JSON artifact messages rather than assuming a target directory or executable
suffix. All twelve CLI invocations, including the expected failing comparison,
use that executable. The CLI build retains the caller's compilation environment.
During the smoke commands only, `CARGO_BUILD_BUILD_DIR` points to a shared
`build` directory inside the invocation's scratch.

Each invocation gets a fresh GUID-named directory under `rust/target`, even
when invoked twice in the same PowerShell process. Final capture and replay
outputs remain separate; compilation intermediates are not shared across
invocations. Cleanup restores the caller's build-directory environment and
removes scratch on success or failure. There is no persistent smoke cache,
lease pool, or new CI cache configuration.

All existing assertions remain: two byte-identical captures, two byte-identical
replays, registry/trace/linked/bundle validation, a successful strict comparison,
independent reference/candidate trace IDs, the expected result `5`, and an
explicit semantic-mismatch path with exit `1` after changing that result to `6`.
`just ctsc-smoke-regressions`, also a prerequisite of `just ctsc-smoke`, covers
executable selection, a single CLI build, command ordering and validator
coverage, distinct scratch per invocation, environment restoration, cleanup,
build/command failures, and both negative-comparison failure modes. Its wrapper
fixtures reuse checked-in CTSC artifacts; the real smoke still exercises the
annotated Rust fixture through capture and replay.

Two serial samples of each `just ctsc-smoke` configuration, collected before the
full gate on the same Windows host with Rust/CTSC sources unchanged from
`d1eca44`, gave:

| Sample | Original recipe | Shared intermediates + one CLI build |
|---|---:|---:|
| First | 208.526 s | 60.369 s |
| Repeated | 200.858 s | 61.474 s |
| **Mean** | **204.692 s** | **60.922 s** |

The observed mean reduction is **143.771 s (70.2%)**, approximately **3.36 times
faster**. Both optimized samples include the new wrapper regressions and start
with empty smoke intermediates. Tool, download, and workspace caches were not
deleted. These are local samples on a shared host, not a statistical benchmark
or an overall gate-speedup claim. The same invocation-local sharing applies in
CI without retaining anything between jobs; CI savings have not been measured.

Subsequent verification illustrates the timing variability: the complete
`just check` passed in **848.303 s**, with its smoke stage taking approximately
**209 s** at the log's one-second timestamp resolution. A standalone smoke
immediately afterward passed in **87.625 s**. There is no paired original-recipe
measurement for those later conditions, and the cause of the variation was not
isolated. The 70.2% reduction describes the initial comparison, not a guarantee
for every run or evidence of an overall gate speedup.

Reproduce with `$env:RUSTUP_TOOLCHAIN = '1.96.0'` and `just ctsc-smoke` from the
repository root. No cache cleanup is needed between samples because each run
allocates and removes its own intermediates.

## Rust workspace tests

After golden regeneration and smoke were optimized, the CLI library's 98 tests
took **346.93 s** within the Windows [#90 CI
run](https://github.com/schgoo/specgate/actions/runs/39444560186), out of
approximately **372 s** for the Rust-test stage. Several capture and replay
tests repeatedly compile the same fixture packages into fresh targets.

An initial serial proof ran seven real Rust capture/replay tests individually,
first with separate intermediates and then with one fresh intermediate directory
shared across those serial processes:

| Case | Separate intermediates | Shared intermediates |
|---|---:|---:|
| Exit inside an open operation | 32.456 s | 31.186 s |
| Preserve a surviving sibling | 33.764 s | 2.517 s |
| Strictly reject an unfinalizable scenario | 32.267 s | 2.806 s |
| Deterministic capture bundle | 33.123 s | 10.858 s |
| Forced capture into occupied output | 32.178 s | 3.062 s |
| Replay contract | 71.036 s | 15.820 s |
| Candidate Cargo configuration | 339.863 s | 47.557 s |

The first build still pays the compilation cost; later captures and replays
reuse dependencies. The two C# end-to-end tests separately took **54.262 s**
and **39.835 s**. Cargo intermediate sharing does not eliminate their
`dotnet` work, and each captured scenario must retain its own test process.

The complete unchanged CLI library test executable was then run twice to
include the cost of giving up outer test parallelism:

| Configuration | CLI library test executable |
|---|---:|
| Original intermediates, default test parallelism | 499.633 s |
| Fresh shared intermediates, `--test-threads=1` | 211.341 s |

Both runs passed **98 tests**, with the same one ignored golden-matrix test.
The observed reduction is **288.292 s (57.7%)**. These are local Windows samples
on the same shared host, not a statistical benchmark, a CI measurement, or a
whole-gate speedup claim. The serial case totals above are not substitutes for
this full-suite comparison.

### Recipe integration

`just rust-test`, included in `just test` and `just check`, applies the measured
policy to the normal `cargo test --workspace` command in `rust`: shared
intermediates plus `-- --test-threads=1`. It skips only the golden harness
unit tests (`goldens::tests::cases`), which `just ctsc-goldens-harness-tests`
runs in parallel; see [CI lanes](#ci-lanes). Keeping Cargo in charge avoids
reimplementing target selection or silently dropping binary, integration, or
doctests. The other product workspace tests also run serially; this is a
deliberate runner policy, not a change to runtime concurrency. Threads and
concurrent operations explicitly exercised inside tests are unaffected.
The separate `test` workspace retains its original command and parallelism.

The initial implementation in #91 allocated a fresh GUID-named
`rust/target/rust-test-*` directory for each invocation and set
`CARGO_BUILD_BUILD_DIR` to its `build` child. This included compilation of the
workspace test targets themselves, not just builds launched inside tests.
Workspace compilation therefore started with empty intermediates even if
`just build` had already run; the following measurements include that cost.

The first integrated `just test` run passed in **298.391 s**, including tool
builds, wrapper regressions, fresh product-workspace compilation, all selected
product tests and doctests, and the separate fixture workspace. Product test
compilation took approximately **66 s**; the CLI library's 98 passing tests took
**194.04 s**. No corresponding original full-recipe measurement was taken under
these conditions, so this is an integration measurement, not a paired
whole-recipe speedup claim.

The subsequent complete `just check` passed in **424.441 s**, including the
unchanged smoke, golden matrix, and C# checks. Host and existing cache state
were not reset, and no paired original full-gate run was taken; this is not
evidence of a particular whole-gate or CI percentage reduction.

### Reusing workspace compilation

The post-merge [#91 main run](https://github.com/schgoo/specgate/actions/runs/39464486084)
still spent **38.95 s** on Linux and **54.47 s** on Windows compiling product
workspace tests into fresh intermediates, after the ordinary workspace build
had already completed.

The [#92 Windows PR run](https://github.com/schgoo/specgate/actions/runs/39596839855)
spent about **81 s** on the same duplicate compilation. Its additional native
C# golden captures and cross-language replay coverage remain unchanged by this
optimization.

The current wrapper scopes the private build directory to Cargo's child-process
environment instead of changing PowerShell's `CARGO_BUILD_BUILD_DIR`. It passes
two Cargo configuration overrides: `env.CARGO_BUILD_BUILD_DIR.value` is the
JSON-quoted absolute scratch path (also a valid TOML string), and
`env.CARGO_BUILD_BUILD_DIR.force` is `true`. Cargo applies its `[env]` table to
children, not to the selection of its own build directory. The workspace test
command therefore reuses the normal workspace cache, including a caller's
existing build-directory override, while tests, doctests, and their nested
Cargo commands inherit invocation-private intermediates.

Generated discovery and replay runners use reusable build namespaces keyed by
the canonical candidate manifest and generated Cargo files. Component-declared
test harnesses use a separate package-scoped namespace. These isolate generated
dependency graphs from ordinary capture builds, which retain shared dependency
compilation. Cargo omits dependency-graph hashes from `cdylib`, `staticlib`, and
`dylib` artifacts; sharing those libraries across incompatible graphs can reuse
a stale rlib and link two runtime instances. Generated builds of ordinary
libraries also need isolation because tracked macro environment inputs do not
change artifact filenames. Package leases prevent overlapping SpecGate commands
from replacing active artifacts without discarding reusable dependencies.

Before #92, a local Windows comparison ran the unchanged `just build test`
recipe first, then the scoped child-environment version, with the same Rust
sources:

| Configuration | Complete `just build test` | Product test compilation | CLI library execution |
|---|---:|---:|---:|
| Private directory also used for workspace compilation | 245.165 s | 49.85 s | 150.81 s |
| Private directory only inherited by children | 191.247 s | 2.08 s | 146.83 s |

Both runs passed the same 98 CLI library tests with one ignored golden-matrix
test, plus the other selected workspace tests and doctests. The optimized
measurement includes the new real-Cargo regressions. The observed complete
recipe reduction is **53.918 s (22.0%)**; workspace compilation alone fell by
**47.77 s**, while CLI execution also varied by **3.98 s**. This is one ordered
pair on a shared host without resetting existing caches, not a statistical
benchmark or a claim that all timing differences are caused by compilation
reuse. CI savings for this follow-up have not been measured.

A subsequent complete root `just check`, also before #92, passed in
**437.557 s**, including the native regressions, unchanged smoke and golden
matrix, and C# checks. This separate gate run is not a paired whole-gate
performance comparison.

After rebasing onto merged #92, the complete root gate passed in **618.290 s**.
Product workspace test compilation took **12.33 s**, and all 111 active CLI
library tests and the new cross-language divergence integration test passed.
The full native C#/Rust golden capture and replay checks remained enabled.
This run includes #92's additional work, so its total is not directly comparable
with the pre-#92 gate timing above; CI savings remain unmeasured.

Cargo runs from the product workspace so its toolchain and configuration lookup
remain unchanged. The wrapper does not change `CARGO_TARGET_DIR`, reuse captured
results, remove assertions, or change candidate runtime resolution. Each test
finishes before a later test can rebuild a generated executable; existing
capture/replay scratch and final-output isolation remain in place. The caller's
environment is never changed; cleanup restores the working directory and removes
the new intermediates. Interrupted scratch is never reused by a later invocation.
There is no persistent cache or new CI cache configuration.

`just rust-test-regressions` checks the exact unfiltered workspace command,
serial execution, workspace working directory, invocation-unique build
directories, preservation of caller target/thread settings and unset/empty/set
build-directory values, and cleanup on success, a nonzero Cargo exit, or a launch
error. A real Cargo fixture also proves that the warmed workspace executable is
used without rebuilding; unit tests and doctests inherit the same private
directory; and two candidate-rooted builds keep separate final targets while
compiling a common dependency only once. It exercises both default and explicit
caller build directories, paths containing spaces, and candidate Cargo
configuration. These regressions are prerequisites of `just rust-test`.

If user Cargo configuration already defines `env.CARGO_BUILD_BUILD_DIR`, it
must use table form. Cargo reports a merge error for a shorthand string entry
when the wrapper supplies its `value` and `force` fields; no fallback silently
disables isolation.

## CI lanes

Baseline: hosted CI ran `just check` as one job per OS. Probe run
[40940991390](https://github.com/schgoo/specgate/actions/runs/40940991390)
(warm Cargo and golden caches, matching main run 40927019974) took 33 min on
`windows-latest` and 20.5 min on `ubuntu-latest`. Windows gate stages:

| Stage | Windows |
|---|---:|
| `build` | 76 s |
| `rust-test` | ~1,050 s |
| `clippy` | 22 s |
| `docs-check` | 51 s |
| `ctsc-smoke` | 128 s |
| `ctsc-goldens-harness-tests` | 16 s compile + 78 s run |
| `ctsc-goldens-check` | 396 s |
| .NET build/test/format | ~60 s |

In the baseline, `rust-test` ran the `goldens::tests::cases` harness tests
serially (~75 s on Windows), duplicating `ctsc-goldens-harness-tests`, which
runs them in parallel. `rust-test` skips them; `just test` and `just check` run
them once.

`just check` is the union of three lane recipes:

| Lane | Recipes |
|---|---|
| `check-rust` | `build`, `rust-test`, `test-workspace` |
| `check-static` | `clippy`, `format-check`, `deny`, `readme-check`, `docs-check`, `dotnet-build`, `dotnet-test`, `format-check-cs` |
| `check-ctsc` | `check-ctsc-smoke` (`ctsc-validate`, `ctsc-smoke`), `check-ctsc-goldens` (`ctsc-goldens-check`) |

CI runs five jobs on each OS, each with its own Cargo cache key:

| CI lane | Recipe | Runs |
|---|---|---|
| `rust-replay` | `check-rust-replay` | `rust-test replay` |
| `rust-other` | `check-rust-other` | `build`, `rust-test other`, `test-workspace` |
| `static` | `check-static` | as above |
| `ctsc-smoke` | `check-ctsc-smoke` | as above, plus `package-smoke` on Ubuntu |
| `ctsc-goldens` | `check-ctsc-goldens` | as above |

`rust-test replay` passes the libtest filter `replay::tests` and
`rust-test other` passes `--skip replay::tests` over the same workspace
selection, so the shards run complementary test sets. Locally, `check-rust`
runs the default `all` shard in one Cargo invocation, so generated runners share
one build directory. Only `ctsc-goldens` restores the golden build cache. A new
gate recipe joins a lane, not `check` directly, so local and CI coverage stay
identical.

Three-lane runs
[40951562582](https://github.com/schgoo/specgate/actions/runs/40951562582)
(cold per-lane Cargo caches; the new keys had no entries) and
[40954323714](https://github.com/schgoo/specgate/actions/runs/40954323714)
(warm):

| Lane | Windows cold | Windows warm | Ubuntu cold | Ubuntu warm |
|---|---:|---:|---:|---:|
| `rust` | 16.3 min | 18.7 min | 7.9 min | 8.5 min |
| `static` | 14.8 min | 4.2 min | 8.3 min | 2.8 min |
| `ctsc` | 14.5 min | 12.8 min | 6.9 min | 9.4 min |
| Critical path | 16.3 min | 18.7 min | 8.3 min | 9.4 min |

With three lanes, the critical path was under 19 min on Windows (baseline 33)
and under 10 min on Ubuntu (baseline 20.5). Hosted runner variance between runs is a few minutes
per lane, larger than the cold/warm difference for `rust` and `ctsc`.
Cold, the Windows `static` lane spent ~9.5 min compiling `cargo-deny` and
`cargo-doc2readme` before its recipes; the Cargo cache retains them.
Five-lane runs
[40985078971](https://github.com/schgoo/specgate/actions/runs/40985078971)
(cold per-lane Cargo caches) and
[40986560850](https://github.com/schgoo/specgate/actions/runs/40986560850)
(warm):

| Lane | Windows cold | Windows warm | Ubuntu cold | Ubuntu warm |
|---|---:|---:|---:|---:|
| `rust-replay` | 8.7 min | 12.0 min | 6.4 min | 6.0 min |
| `rust-other` | 13.0 min | 11.6 min | 5.5 min | 5.1 min |
| `static` | 14.7 min | 4.7 min | 5.7 min | 2.3 min |
| `ctsc-smoke` | 4.2 min | 3.8 min | 4.4 min | 4.7 min |
| `ctsc-goldens` | 8.8 min | 6.8 min | 6.3 min | 5.7 min |
| Critical path | 14.7 min | 12.0 min | 6.4 min | 6.0 min |

With warm caches, the five-lane critical path is 12.0 min on Windows (three
lanes 18.7, baseline 33) and 6.0 min on Ubuntu (three lanes 9.4, baseline
20.5). Each Rust shard compiles the workspace tests, builds the C# tools, and
builds its own generated runners; Windows `rust-other` also spends ~2 min in
`build`.
Lanes use more runner minutes; GitHub does not bill them for public
repositories.

Persisting the `rust-test` child build directory across CI runs was measured
and rejected. Locally it cut `rust-test` from 1,137 s cold to 586 s warm, but
the directory reached 11.2 GB on Windows (debug 1.8 GB plus 19 harness and 22
runner namespaces). It would exceed GitHub's 10 GB per-repository cache limit
and evict the other caches.

## Reproduction

For complete gate logs, run from the repository root:

```powershell
$env:RUSTUP_TOOLCHAIN = '1.96.0'
just --timestamp --verbose check
```

CI uses the same logging flags, running each CI lane
(`just check-rust-replay`, `just check-rust-other`, `just check-static`,
`just check-ctsc-smoke`, `just check-ctsc-goldens`) as a separate job. No
recipe is omitted; `just check` runs the three local lanes in sequence.

To compare the golden harness directly, start a fresh PowerShell process from
the repository root. Run the following once with `$shared = $false`, then
again with `$shared = $true`. Repeat both for warm-host-cache samples.

```powershell
$env:RUSTUP_TOOLCHAIN = '1.96.0'
just discovery-tool
$env:SPECGATE_DISCOVERY_PATH = (Resolve-Path artifacts\discovery).Path
$messages = @(cargo test --manifest-path rust\Cargo.toml -p specgate-cli `
    --lib --no-run --quiet --message-format=json)
if ($LASTEXITCODE -ne 0) { throw 'Harness build failed.' }
$harness = $messages | ConvertFrom-Json | Where-Object {
    $_.reason -eq 'compiler-artifact' -and $_.profile.test -and
    $_.target.name -eq 'specgate_cli' -and $_.target.kind -contains 'lib' -and
    $_.executable
} | Select-Object -ExpandProperty executable -Unique
if (@($harness).Count -ne 1) { throw 'Expected one harness executable.' }

$shared = $false
$scratch = Join-Path ([IO.Path]::GetTempPath()) "sg-bench-$([guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory $scratch | Out-Null
$env:SPECGATE_CACHE_DIR = Join-Path $scratch 'cache'
$env:SPECGATE_CTSC_GOLDENS = 'check'
$env:SPECGATE_CTSC_GOLDENS_SCRATCH = $scratch
if ($shared) {
    $env:CARGO_BUILD_BUILD_DIR = Join-Path $scratch 'build'
} else {
    Remove-Item Env:CARGO_BUILD_BUILD_DIR -ErrorAction SilentlyContinue
}
try {
    $elapsed = Measure-Command {
        & $harness --ignored --exact --nocapture `
            'goldens::tests::golden_matrix' | Out-Host
        if ($LASTEXITCODE -ne 0) { throw 'Golden harness failed.' }
    }
    $elapsed.TotalSeconds
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}
```

Use a fresh process so the example's environment overrides do not affect later
work. The normal golden recipes save and restore the caller's environment,
including an existing `CARGO_BUILD_BUILD_DIR`.

For the retained-cache recipe comparison, run `just ctsc-goldens-update` twice
serially from the same worktree and toolchain, starting with the named
intermediate cache absent. Measure each entire recipe with a stopwatch. Do not
remove any directory while a golden wrapper or its surviving children are
using it. To reproduce the direct retained-cache experiment, keep
`$shared = $true` and point
`CARGO_BUILD_BUILD_DIR` at one dedicated directory outside `$scratch` across
the three runs; only `$scratch` is removed between runs. Do not share that
experimental directory with concurrent processes.

## Other options

| Option | Decision |
|---|---|
| Share final `CARGO_TARGET_DIR` across all generated runners | Not needed: sharing intermediates delivers the measured saving while retaining final-output isolation. |
| Persist the golden build cache across invocations | Implemented after measuring the follow-up: exclusive cache slots preserve concurrency isolation; only successful completion permits reuse, while interrupted or failed slots stay quarantined. Disk reclamation is explicit. |
| Reuse C# assemblies or `--artifacts-path` across invocations | Deferred: C# discovery is already batched; preserve real-project/source-generator compilation and isolated outputs. |
| Parallelize full-gate stages | Implemented in CI as [lanes](#ci-lanes) on separate runners, which avoids CPU and build-lock contention; `just check` runs the lanes sequentially. |
| nextest/test partitioning | Not the golden bottleneck: golden capture already executes enumerated ordinary tests through its existing isolation mechanism. Workspace test acceleration is separate work. |
| `check-fast` or skip replay/validation | Not introduced: the requested improvement is faster regeneration, not less evidence. |

Any further optimization must retain exact source/component inventory,
strict test capture, operation coverage, negative diagnostic attribution,
parity, bundle/link validation, replay categories, and byte comparison.
