//! True self-hosting: `run_spec` runs the real harness spec.
//!
//! `specs/specgate.harness.spec.yaml` is bound to the `specgate-selfhost` crate,
//! whose `#[spec_operation("run_spec")]` wrapper exposes the harness's own
//! `run_spec` as a spec operation. Running the harness against its own spec
//! validates the harness end-to-end through its own pipeline: each `run_spec`
//! case runs a fixture and asserts the structured `$result` (outcome variant,
//! per-result name/status/expected/traces); command cases run their binding
//! command; narratives are skipped.
//!
//! This is doubly-nested (the outer `run_spec` builds a runner that itself calls
//! `run_spec` per case, plus command targets shell out to cargo), so it is slow.
//! Run explicitly with `--ignored`.

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
    let spec = root.join("specs/specgate.harness.spec.yaml");
    match run_spec(spec.to_str().expect("utf-8 path")) {
        RunOutcome::Error { reason } => panic!("self-host run errored: {reason}"),
        RunOutcome::Complete { results } => {
            // Narratives skip; everything else must pass. Only Fail is a failure.
            let failed: Vec<&str> = results
                .iter()
                .filter(|r| r.status == CaseStatus::Fail)
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
