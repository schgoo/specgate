//! True self-hosting for the CLI spec: run the harness against
//! `specs/specgate.cli.spec.yaml`, exercising the CLI's own `validate` and
//! `run` operations (annotated `#[spec_operation]` in this crate) and asserting
//! the structured `$result`. Replaces hand-written CLI integration tests.
//!
//! Doubly-nested for `run` cases (the CLI `run` op itself runs a spec), so this
//! is slow and `#[ignore]`d; invoke with `--ignored` (via `just cli-self-host`).

use specgate_harness::{CaseStatus, RunOutcome, run_spec};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate-cli
    p.pop(); // crates
    p.pop(); // rust
    p.pop(); // repo root
    p
}

#[test]
#[ignore = "doubly-nested CLI self-host run is slow; invoke with --ignored"]
fn cli_spec_self_hosts() {
    let root = repo_root();
    let spec = root.join("specs/specgate.cli.spec.yaml");
    match run_spec(spec.to_str().expect("utf-8 path")) {
        RunOutcome::Error { reason } => panic!("CLI self-host run errored: {reason}"),
        RunOutcome::Complete { results } => {
            let failed: Vec<&str> = results
                .iter()
                .filter(|r| r.status == CaseStatus::Fail)
                .map(|r| r.name.as_str())
                .collect();
            assert!(
                failed.is_empty(),
                "{} CLI self-host cases failed (of {}): {:?}",
                failed.len(),
                results.len(),
                failed
            );
        }
    }
}
