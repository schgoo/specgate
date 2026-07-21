//! Minimal engine self-test fixture. A trivial free-function operation used by
//! the engine spec (`specgate.harness`) as a known-good spec for the
//! `self_test_rejects_all_errors` case. Rust-only — it deliberately has no C#
//! conformance binding; it exists purely to prove the harness self-test does
//! not pass vacuously when every case is expected to complete.
use specgate::*;

#[spec_operation("probe")]
pub fn probe() -> i32 {
    1
}
