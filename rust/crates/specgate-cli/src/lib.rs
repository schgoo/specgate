//! Command-line interface for [SpecGate](https://github.com/schgoo/specgate):
//! validate specs, run them through the harness, extract specs from
//! annotated code, discover implementation metadata as CTSC registries, and
//! capture passing Rust tests as native CTSC reference bundles.
//! This library backs the `specgate` binary and the integration-test suite;
//! each command is also callable as a function (`validate`, `run`, `extract`,
//! `discover`, `capture`).
//!
//! # Commands
//!
//! ```text
//! specgate validate <spec-dir> [--strict] [--spec-only] [--assertions-dir <dir>]
//! specgate run <spec.yaml> [--coverage] [--coverage-threshold <pct>] [--verbose] [--json]
//! specgate extract <package-root> -o|--out <spec.yaml> [--component <name>] [--cases]
//! specgate discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json> [--target <name>]
//! specgate capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]
//! ```
//!
//! ## `validate`
//!
//! Checks every spec under `<spec-dir>` against the schema, then runs
//! runnability checks that mirror the hard errors the harness would raise (no
//! cases, an unresolvable binding, an unknown target, a missing
//! `package_root`, an operation with no `#[spec_operation]`, unwireable
//! setups, non-`pub` setups/input types).
//!
//! - `--spec-only` — skip checks that need the implementation source (for
//!   authoring a spec before the code exists).
//! - `--strict` — treat warnings as errors.
//! - `--assertions-dir <dir>` — directory of source-assertion files to
//!   cross-check provenance against.
//!
//! Exit code `0` on pass, `1` on failure.
//!
//! ## `run`
//!
//! Generates, builds, and runs the harness for a single spec, reporting
//! per-case pass/fail.
//!
//! - `--coverage` — measure the implementation crate's code coverage.
//! - `--coverage-threshold <pct>` — fail the run if coverage falls below
//!   `<pct>` (implies `--coverage`).
//! - `--verbose` — include passing cases in the human-readable case list.
//! - `--json` — emit the full structured run report as JSON.
//!
//! Exit code `0` when all cases pass, `1` on any failure or error.
//!
//! ## `extract`
//!
//! Derives a `.spec.yaml` (plus a sibling binding file) from an annotated
//! crate — the reverse of implementing a spec. By default only the schema
//! (operations, inputs/outputs, types) is derived, leaving `cases:` empty;
//! with `--cases`, the crate's existing tests are run under record mode and
//! each passing test is captured as a case.
//!
//! - `-o`, `--out <spec.yaml>` — output path for the derived spec (required).
//! - `--component <name>` — which component to extract (required when the
//!   crate hosts more than one).
//! - `--cases` — also capture runnable cases from the crate's tests.
//!
//! Extraction is deterministic and uses no LLM.
//!
//! ## `discover`
//!
//! Loads one target from a binding, invokes its existing language-specific
//! discovery mechanism (Rust link-time registration or C# reflection), asks
//! the harness for its normalized, setup-folded schema, and writes a compact
//! deterministic CTSC registry document.
//!
//! - `--component <id>` — component whose operations to encode (required).
//! - `--registry-id <id>` — CTSC registry identifier (required).
//! - `--registry-version <version>` — registry version (required).
//! - `-o`, `--out <registry.ctsc.json>` — output path (required).
//! - `--target <name>` — binding target; omitted selects the default.
//!
//! ## `capture`
//!
//! Runs every libtest test in isolation for one Rust binding target. Passing
//! tests that invoke the selected component become ordered native CTSC
//! scenarios; tests that do not invoke it are omitted. The deterministic output
//! directory contains exactly `registry.ctsc.json`, `reference.otlp.json`, and
//! `manifest.json`.
//!
//! - `--out <dir>` — output directory for the bundle (required).
//! - `--target <name>` — binding target; omitted selects the default.
//! - `--component <id>` — component to capture; omitted selects the sole
//!   discovered component and errors when discovery is ambiguous.

// The crate-root default component for all annotated items in this crate's
// submodules (extract/run/validate). Submodules reference the generated
// `crate::__SPECGATE_COMPONENT` constant.
specgate::spec_component!("specgate.cli");

pub mod capture;
pub mod discover;
pub mod extract;
pub mod run;
pub mod validate;

pub use capture::{CaptureOutcome, CaptureReport, capture};
pub use discover::{DiscoverOutcome, DiscoverReport, discover};
pub use extract::{ExtractOutcome, ExtractReport, extract};
pub use run::{CaseReport, RunOutcome, RunReport, TargetDivergence, run};
pub use validate::{Severity, ValidateOutcome, ValidationFinding, ValidationReport, validate};
