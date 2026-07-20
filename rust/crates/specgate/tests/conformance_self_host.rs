//! Cross-language conformance self-host: `run_spec` runs the conformance spec.
//!
//! `specs/specgate.conformance.spec.yaml` is the conformance ledger — each case
//! runs a dual-bound fixture and asserts every bound target emits a
//! byte-identical canonical trace. Like the engine self-host it is bound to the
//! `specgate-selfhost` crate's `run_spec` wrapper, so running it validates the
//! harness's multi-target comparison path end-to-end through its own pipeline.
//!
//! Doubly-nested (the outer `run_spec` builds a runner that itself calls
//! `run_spec` per case, and dual-bound cases additionally build+run the C#
//! backend), so it is slow. Run explicitly with `--ignored`.

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
fn conformance_spec_self_hosts() {
    let root = repo_root();
    let spec = root.join("specs/specgate.conformance.spec.yaml");
    match run_spec(spec.to_str().expect("utf-8 path")) {
        RunOutcome::Error { reason } => panic!("conformance self-host run errored: {reason}"),
        RunOutcome::Complete { results } => {
            let failed: Vec<&str> = results
                .iter()
                .filter(|r| r.status == CaseStatus::Fail)
                .map(|r| r.name.as_str())
                .collect();
            assert!(
                failed.is_empty(),
                "{} conformance self-host cases failed (of {}): {:?}",
                failed.len(),
                results.len(),
                failed
            );
        }
    }
}
