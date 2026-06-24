//! True self-hosting: `run_spec` runs the self-host form of the harness spec.
//!
//! `selfhost_harness.spec.yaml` is the executable, self-host rendering of the
//! `run_spec` cases documented in `specs/specgate.harness.spec.yaml`: every case
//! invokes the `run_spec` operation (the `selfhost.rs` wrapper) on a fixture
//! spec and asserts the structured `$result` (outcome variant, per-result
//! name/status, and full inner traces). This test runs the harness against that
//! spec through the public API — the harness validating itself end-to-end.
//!
//! This is doubly-nested (the outer `run_spec` builds a runner that itself calls
//! `run_spec` per case), so it is slow. Run explicitly with `--ignored`.

use specgate::{CaseStatus, RunOutcome, run_spec};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate
    p.pop(); // crates
    p.pop(); // rust
    p.pop(); // repo root
    p
}

#[test]
#[ignore = "doubly-nested self-host run is slow; invoke with --ignored"]
fn harness_spec_self_hosts() {
    let root = repo_root();
    let spec = root.join("test/rust/crates/specgate-fixtures/specs/selfhost_harness.spec.yaml");
    match run_spec(spec.to_str().expect("utf-8 path")) {
        RunOutcome::Error { reason } => panic!("self-host run errored: {reason}"),
        RunOutcome::Complete { results } => {
            let failed: Vec<&str> = results
                .iter()
                .filter(|r| r.status != CaseStatus::Pass)
                .map(|r| r.name.as_str())
                .collect();
            assert!(
                failed.is_empty(),
                "{} self-host cases failed (of {}): {:?}",
                failed.len(),
                results.len(),
                failed
            );
        }
    }
}
