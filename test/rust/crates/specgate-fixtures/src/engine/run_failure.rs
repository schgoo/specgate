//! Minimal single-failing-case vehicle for the CLI `run_with_failure` spec.
//! Owns its own operation under `fixture.run_failure` so the CLI test has a
//! deterministic 1-case / 0-pass / 1-fail report to assert on.
use specgate::*;

/// Returns a constant; the spec asserts a wrong value so the case fails.
#[spec_operation("emit", spec = "fixture.run_failure")]
pub fn emit() -> i32 {
    1
}
