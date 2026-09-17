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
    ensure_replay_capture_fixture(&root);
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

fn ensure_replay_capture_fixture(root: &std::path::Path) {
    let capture_dir = root.join("rust").join("target").join("ctsc-capture-stateless");
    let complete = ["manifest.json", "registry.ctsc.json", "reference.otlp.json"]
        .iter()
        .all(|file| capture_dir.join(file).is_file());
    if complete {
        return;
    }
    let binding = root
        .join("test")
        .join("rust")
        .join("crates")
        .join("specgate-fixtures")
        .join("specs")
        .join("binding.yaml");
    let outcome = specgate_cli::capture(
        binding.to_str().expect("utf-8 binding path"),
        "",
        "fixture.stateless_add",
        capture_dir.to_str().expect("utf-8 capture path"),
    );
    assert!(
        matches!(outcome, specgate_cli::CaptureOutcome::Complete { .. }),
        "failed to prepare replay capture fixture: {outcome}"
    );
}
