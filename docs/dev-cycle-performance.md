# Dev-cycle performance

Measurements for [#87](https://github.com/schgoo/specgate/issues/87), collected
on 2026-10-19 and 2026-10-20. The optimizations target compilation reuse, not a
reduced gate.
`just check` retains its original recipes and order.

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
and the existing lease/quarantine protocol is unchanged.

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

## Reproduction

For complete gate logs, run from the repository root:

```powershell
$env:RUSTUP_TOOLCHAIN = '1.96.0'
just --timestamp --verbose check
```

CI uses the same logging flags. No recipe is omitted or parallelized.

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
| Parallelize full-gate stages | Deferred: first remove redundant compilation; parallel Cargo/MSBuild processes can contend for CPU and build locks. |
| nextest/test partitioning | Not the golden bottleneck: golden capture already executes enumerated ordinary tests through its existing isolation mechanism. Workspace test acceleration is separate work. |
| `check-fast` or skip replay/validation | Not introduced: the requested improvement is faster regeneration, not less evidence. |

Any further optimization must retain exact source/component inventory,
strict test capture, operation coverage, negative diagnostic attribution,
parity, bundle/link validation, replay categories, and byte comparison.
