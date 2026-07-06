# Specgate-Cli

[![crates.io](https://img.shields.io/crates/v/specgate-cli.svg)](https://crates.io/crates/specgate-cli)
[![docs.rs](https://docs.rs/specgate-cli/badge.svg)](https://docs.rs/specgate-cli)
[![CI](https://github.com/schgoo/specgate/actions/workflows/ci.yml/badge.svg)](https://github.com/schgoo/specgate/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

Command-line interface for [SpecGate][__link0]:
validate specs, run them through the harness, and extract specs from
annotated code. This library backs the `specgate` binary and the
integration-test suite; each command is also callable as a function
(`validate`, `run`, `extract`).

## Commands

```text
specgate validate <spec-dir> [--strict] [--spec-only] [--assertions-dir <dir>]
specgate run <spec.yaml> [--coverage] [--coverage-threshold <pct>]
specgate extract <package-root> -o|--out <spec.yaml> [--component <name>] [--cases]
```

### `validate`

Checks every spec under `<spec-dir>` against the schema, then runs
runnability checks that mirror the hard errors the harness would raise (no
cases, an unresolvable binding, an unknown target, a missing
`package_root`, an operation with no `#[spec_operation]`, unwireable
setups, non-`pub` setups/input types).

* `--spec-only` — skip checks that need the implementation source (for
  authoring a spec before the code exists).
* `--strict` — treat warnings as errors.
* `--assertions-dir <dir>` — directory of source-assertion files to
  cross-check provenance against.

Exit code `0` on pass, `1` on failure.

### `run`

Generates, builds, and runs the harness for a single spec, reporting
per-case pass/fail.

* `--coverage` — measure the implementation crate’s code coverage.
* `--coverage-threshold <pct>` — fail the run if coverage falls below
  `<pct>` (implies `--coverage`).

Exit code `0` when all cases pass, `1` on any failure or error.

### `extract`

Derives a `.spec.yaml` (plus a sibling binding file) from an annotated
crate — the reverse of implementing a spec. By default only the schema
(operations, inputs/outputs, types) is derived, leaving `cases:` empty;
with `--cases`, the crate’s existing tests are run under record mode and
each passing test is captured as a case.

* `-o`, `--out <spec.yaml>` — output path for the derived spec (required).
* `--component <name>` — which component to extract (required when the
  crate hosts more than one).
* `--cases` — also capture runnable cases from the crate’s tests.

Extraction is deterministic and uses no LLM.


---

Part of the [SpecGate](https://github.com/schgoo/specgate) project.

 [__link0]: https://github.com/schgoo/specgate
