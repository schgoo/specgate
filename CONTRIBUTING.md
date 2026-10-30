# Contributing to SpecGate

## Setup

1. Install the pinned Rust toolchain from `rust/rust-toolchain.toml`.
2. Install `just`, `cargo-deny`, and `cargo-doc2readme`.
3. Install Python dependencies from `docs/ctsc/requirements.txt`.
4. Install the .NET 10 SDK.

## CTSC-first development

Behavior changes must update or add focused registry, native trace, capture, or
replay tests. Capture behavior at real operation boundaries through ordinary
tests; do not introduce a parallel assertion language or flat trace sink.

Use `binding-schema.json` for Rust/C# target bindings. The active CLI commands
are `discover`, `capture`, `replay`, `validate`, and `compare`.

## Planning and agent workflow

Start with `AGENTS.md` and `docs/README.md`. Use the observe → plan → act →
verify loop in
[`docs/contributor/agentic-loop.md`](docs/contributor/agentic-loop.md): define
one bounded vertical slice with explicit responsibility boundaries, implement it through the existing CTSC
mechanisms, then independently inspect the diff and validation evidence.

Normative CTSC shapes and validation, public CLI shape, capture/replay identity
and stimulus semantics, comparison policy semantics, runtime package identity,
and golden-matrix declaration meaning require an explicit human owner. Record
ratified architectural decisions under `docs/decisions/`.

## Golden matrix

`test/behavior/matrix.json` is the reviewable configuration for the whole
fixture corpus. Edit it by hand when you add, remove, or reclassify a fixture:
every annotated fixture source must belong to a row, every discovered component
needs a row, and each row states its language phases, expected outcome, parity
mode, linkage mode, reference-to-candidate replay checks, and any limitation. Linkage is `linked` for
every row that captures a bundle and `not-applicable` for discovery-only rows;
a bundle that will not link is a defect to fix, not a row to annotate. A
`parity` exception must also list every allowed Rust-vs-C# difference as a JSON
Pointer plus the exact value each language emits; the harness requires the
observed difference set to equal the declared one.

Matrix `0.2.0` has no compatibility reader. Each captured component selects
exactly one successful discover+capture endpoint through `reference:
{ implementation, artifacts }`. Its artifacts are exactly `manifest.json`,
`reference.otlp.json`, and `registry.ctsc.json`, directly under `cases/<id>/expected/`.
There are no per-language successful-component artifacts, including registries.
A discovery-only component selects a successful discovery endpoint and keeps
only `cases/<id>/expected/registry.ctsc.json`. Keep both
native capture phases: nonselected producers capture strictly into invocation
scratch and undergo the same native validation and exact coverage checks.
Never copy or relabel traces or validate a producer's native bundle against
another endpoint's registry. In check mode, anchor the selected full native
registry's bytes to the checked-in canonical, then compare every other
independently generated registry to that same oracle with exact equality or the
existing exact `parity.allowedDifferences`. Preserve full dependency closure and
ordering and Rust/C# difference orientation regardless of endpoint selection.
Fresh-producer-only comparison is insufficient: both producers could drift
together. Exact exceptions describe known non-equivalences, not blanket passes.
The selected producer writes directly into the case's `expected/` directory, with no
`reference` subdirectory. Native `target.name` is that directory's basename;
its language and native scenario names remain unchanged.
Captured rows require checks, all naming the selected reference. Discovery-only
components have a registry-only reference and no checks; negative and replacement
rows have neither reference nor checks. The fixture
configuration selects C# for paired captured rows and Rust for Rust-only rows;
this does not impose a harness language policy.

The check runs in both directions: the components each compiled target actually
declares must be exactly the components the matrix rows name, and every
enumerated fixture test must pass under golden capture. Every discovered
operation must be exercised by a captured trace (nested calls count), unless
the row's `captureExclusions` names the exact operation with a stable code and
reason. Replay checks name implementation endpoints, not languages. C# native
bundles replay on Rust candidates where supported; replay limitations never
remove native capture coverage. Unsupported checks declare the exact `expectCategory`;
an unrelated planner or discovery error never satisfies them. Build negatives
reject any compiler error without primary spans exclusively in the row's
intentional sources, even when the intended error is also present.

Only declared case and rejection `expected/` directories are generated. Do not hand-edit them.

```powershell
just ctsc-goldens-update   # regenerate artifacts after an intended change
just ctsc-goldens-check    # regenerate into scratch and byte-compare (part of just check)
```

After an intentional change, run `just ctsc-goldens-update`, review the
artifact diff as carefully as the code diff, then confirm
`just ctsc-goldens-check` passes.

Both recipes reuse Cargo intermediate build artifacts under
`rust/target/ctsc-goldens-build-cache` (CI: `SPECGATE_CTSC_GOLDENS_BUILD_CACHE`).
No golden output or validation result is cached; every run executes the entire
matrix. Remove that directory only after every golden wrapper, harness, and
child process has exited. Slot leasing, quarantine, and CI retention are
described in
[`docs/contributor/dev-cycle-performance.md`](docs/contributor/dev-cycle-performance.md#golden-regeneration).

## Gate

Run from the repository root:

```powershell
just check
```

`just check` runs the same recipes as CI. First, `check-prepare` builds the
Rust workspaces, the C# tools, and the solution one time, because these builds
share project `obj` directories. Then `check-lanes` runs four local lanes in
parallel:

- `check-rust-replay`: the `replay` Rust test shard.
- `check-local-rust-isolated`: build, the `isolated` Rust test shard, and the
  `test` workspace.
- `check-static`: clippy, formatting, licenses, READMEs, docs, and the C#
  build/tests/format.
- `check-local-fixture`: `check-ctsc-smoke`, `check-ctsc-goldens`, and then the
  `shared-fixture` Rust test shard. These recipes lease the same golden-matrix
  fixture package, and a busy lease fails immediately, so they run in sequence.

CI runs five parallel jobs on each OS: `check-rust-replay`, `check-rust-other`,
`check-static`, `check-ctsc-smoke`, and `check-ctsc-goldens`. `check-rust` and
`check-ctsc` run the Rust and CTSC halves in sequence. Add a new gate recipe to
one CI lane and to the local lane that runs that CI lane, not directly to
`check`.

Rust tests that drive C# need the prebuilt discovery and weaver tools. `just test`
builds them and exports `SPECGATE_DISCOVERY_PATH` and `SPECGATE_WEAVER_PATH`; to run
`cargo test` directly, run `just discovery-tool weaver-tool` first and export the
two variables as the `justfile` does (`artifacts/discovery`, `artifacts/weaver`).

`just rust-test` runs the product Rust workspace through Cargo with one outer
test thread. Cargo's child-process environment gives tests and doctests one
shared intermediate build directory. This lets capture/replay tests reuse
dependencies without overlapping rebuilds and execution of generated binaries.
`SPECGATE_RUST_TEST_BUILD_DIR` selects where the directories are:

- `persistent` (local default): each shard owns a target directory and a child
  build directory under `rust/target/rust-test/<shard>`. The wrapper keeps
  them, so later runs reuse warm builds. Parallel shards do not relink an
  executable that another lane runs. Each shard directory can use 6–10 GB;
  `cargo clean` in `rust` removes them.
- `scratch` (CI): workspace compilation uses the normal target directory and
  reuses artifacts from `just build`. Child intermediates go to a fresh
  `rust/target/rust-test-<guid>/build`, and the wrapper removes it.

Cargo still selects the full workspace's unit, binary, integration, and doctests;
threads explicitly created inside tests are unaffected. Wrapper regressions run
first, including a real Cargo fixture proving warm executable reuse and shared
dependency compilation across isolated candidate targets. The caller's
environment is unchanged; cleanup restores the working directory and removes
scratch.
`just rust-test` skips the golden harness unit tests (`goldens::tests::cases`),
which `just ctsc-goldens-harness-tests` runs in parallel. `just rust-test
replay` runs only tests whose names contain `replay::tests`, and
`just rust-test other` runs the rest; CI uses these complementary shards, while
the default `all` runs both in one Cargo invocation. The local `isolated` and
`shared-fixture` shards split `other`: `shared-fixture` runs only tests in a
`shared_fixture` module. Put a Rust test in a `shared_fixture` module when it
runs a SpecGate command on a `test/behavior` fixture package. `just test` includes
that recipe and also runs the separate `test` workspace with its original Cargo
command and parallelism.

The runner owns the `value` and `force` entries of Cargo's
`env.CARGO_BUILD_BUILD_DIR` table for its child processes. If custom Cargo
configuration defines that entry, use table form instead of the shorthand
string form: Cargo rejects merging a string with the runner's table entries.

The gate covers Rust build/tests/clippy/fmt/licenses, generated crate READMEs,
the public docs (`just docs-check`), the CTSC Python corpus and linked
validators, a deterministic capture-to-replay smoke, the CTSC golden matrix
check, and the retained C# build/tests/format/analyzers.

`just docs-check` fails when the docs drift from the code. It checks:

- the CLI usage block in [`docs/reference/cli.md`](docs/reference/cli.md)
  against the program's usage text;
- relative links and heading anchors in repository Markdown
  (`scripts/docs-links.ps1`);
- the diagnostic fragments quoted in
  [`docs/guides/troubleshoot-capture.md`](docs/guides/troubleshoot-capture.md)
  against non-test sources (`scripts/docs-diagnostics.ps1`);
- every `specgate.yaml` in the repository, and every documentation YAML block
  whose first line is `# yaml-language-server: $schema=<relative path>/binding-schema.json`,
  against `binding-schema.json` and the binding loader
  (`rust/crates/specgate-discovery/tests/binding_examples.rs`);
- the [documentation impact](#documentation-impact) map
  (`scripts/docs-impact.ps1`).

Each check runs its own regression tests, which prove it fails on bad input.

`just ctsc-smoke` first runs `just ctsc-smoke-regressions`, builds the CLI once,
and shares Cargo intermediates between its capture and replay commands.
Intermediates live only in the invocation's GUID-named
`rust/target/ctsc-smoke-*` scratch directory; separate runs never reuse it.
Capture/replay outputs remain separate, every original assertion still runs,
and cleanup restores the caller's `CARGO_BUILD_BUILD_DIR`. There is no persistent
smoke cache to warm locally or restore in CI.

Use `just --timestamp --verbose check` to identify recipe boundaries in a gate
log; CI uses the same flags without changing the checks being run. See
[`docs/contributor/dev-cycle-performance.md`](docs/contributor/dev-cycle-performance.md) for the
stage breakdown, compilation-reuse measurements, and reproduction commands.

Run `just package-smoke` for release changes; it packages all seven workspace crates,
installs the packaged CLI, and exercises registry-dependency
discover/capture/replay.

`cargo-evaluate` is an internal local review tool and is not part of
`just check` or GitHub-hosted CI. After installing `cargo-evaluate 0.23.0`, use
`just evaluate-offline` for the fast deterministic rules, `just
evaluate-cached` for quiet cache-aware semantic review with an authenticated
Copilot CLI, and `just evaluate` for the same complete audit with console
findings. The evaluator reuses entries only while both file context and rule
definition hashes match. Exact justified exceptions live in
`rust/evaluate.toml`.

Use Conventional Commits and keep changes scoped. The changelog is generated
from commit subjects, so the subject is the entry a reader sees.

## Documentation impact

Behavior changes must update every page that describes the behavior.
[`docs/contributor/docs-impact.json`](docs/contributor/docs-impact.json)
assigns every non-test product source to the pages that describe it. A rule
with no pages gives a `reason`. `just docs-check` fails when a source is
uncovered, a glob matches nothing, or a listed page is missing.

The pull-request CI job runs [`scripts/docs-impact.ps1`](scripts/docs-impact.ps1)
against the base branch; run `just docs-impact` locally before opening a pull
request. A rule fails when its code changed but none of its pages did. If you
checked every listed page and none needs a change, add a commit trailer:

```text
Docs-Impact: none <reason>
```

The map only proves that a page was touched, not that its prose is true. When
behavior changes, also search the docs for the old rule's wording; limits are
often restated outside the mapped pages.

## Agent plugin

`plugins/specgate/` is an agent plugin (Claude Code and Copilot CLI) that
teaches an agent to install and use SpecGate in its own project, without the
contributor rules in this repository. It is generated; do not edit it by hand.

[`docs/contributor/agent-plugin.json`](docs/contributor/agent-plugin.json)
lists the bundled guide and reference pages and maps each skill to the guides
it contains. [`scripts/agent-plugin.ps1`](scripts/agent-plugin.ps1) copies the
bundle under `plugins/specgate/docs/`, builds one `SKILL.md` per skill, and
reads the plugin version from `rust/Cargo.toml`. It strips check tags and
`Source of truth` sections and rewrites links to files outside the bundle to
GitHub URLs pinned to `v<version>`, so those links resolve once that release
is tagged.

Run `just agent-plugin` after changing a bundled page, the manifest, or the
workspace version. `just docs-check` fails when the committed plugin differs
from a fresh build, when a link or anchor in it does not resolve, or when a
`SKILL.md` reaches 500 lines.

## Changelog

`CHANGELOG.md` entries are one line per change. `just changelog` previews
entries for commits since the last `vX.Y.Z` tag, grouped by commit type;
`pwsh scripts/changelog.ps1 -Version <x.y.z> -Apply` writes a release section
and folds any hand-curated `## [Unreleased]` body in above the generated
bullets.

Rationale does not belong in the changelog. Put it in the commit body, an ADR
under `docs/decisions/`, or `docs/contributor/roadmap.md`.