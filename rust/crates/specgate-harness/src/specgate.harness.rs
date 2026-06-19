//! Fixture file for `specs/specgate.harness.spec.yaml`.
//!
//! This file is **not** part of the `specgate-harness` library build —
//! cargo only compiles files referenced from `lib.rs`. The harness
//! `resolve_fixture_source` looks here when running the harness spec
//! against itself: it scans for `#[spec_operation]` annotations and
//! includes this file (via `#[path]`) in a generated runner project.
//!
//! The annotated `run_spec` here is a thin wrapper that delegates to the
//! real implementation in `specgate_harness::run_spec` so the generated
//! runner exercises the actual library code. `mechanism_proof` and
//! `check_fixtures` are `kind: command` operations whose harness cases
//! only assert `outcome.Complete: {}` (which the harness treats as an
//! empty assertion list — pass on non-panicking execution); the
//! companion `cargo test --test mechanism_proof` and `cargo check` runs
//! that those cases describe are exercised separately by the workspace
//! CI, not from inside this self-test, to avoid recursive cargo builds
//! deadlocking on file locks.

use specgate_annotations::*;

#[spec_operation("run_spec")]
pub fn run_spec(spec: &str) -> specgate_harness::RunOutcome {
    specgate_harness::run_spec(spec)
}

#[spec_operation("mechanism_proof")]
pub fn mechanism_proof() {}

#[spec_operation("check_fixtures")]
pub fn check_fixtures() {}
