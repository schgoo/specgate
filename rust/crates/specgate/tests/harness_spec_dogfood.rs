//! Dogfood: the harness spec validates itself through the public API.
//!
//! `specs/specgate.harness.spec.yaml` documents, for each `run_spec` case, the
//! `RunOutcome` the harness should produce for a referenced fixture spec. This
//! test makes that documentation executable: it runs each referenced fixture
//! through the public `specgate::run_spec` and asserts the real outcome matches
//! what the spec claims (outcome variant + per-result pass/fail status).
//!
//! There is no skip list: every `run_spec` case in the harness spec must hold.

use specgate::{RunOutcome, run_spec};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR")); // rust/crates/specgate
    p.pop(); // crates
    p.pop(); // rust
    p.pop(); // repo root
    p
}

#[test]
fn harness_spec_documented_outcomes_match() {
    let root = repo_root();
    let spec_path = root.join("specs/specgate.harness.spec.yaml");
    let text = std::fs::read_to_string(&spec_path).expect("read harness spec");
    let doc: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse harness spec");
    let cases = doc
        .get("cases")
        .and_then(serde_yaml::Value::as_sequence)
        .expect("harness spec has cases");

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;

    for case in cases {
        let name = case.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if case.get("operation").and_then(|o| o.as_str()) != Some("run_spec") {
            continue;
        }
        let Some(spec_rel) = case.get("inputs").and_then(|i| i.get("spec")).and_then(|s| s.as_str()) else {
            continue;
        };
        let Some(outcome) = case.get("expected").and_then(|e| e.get("outcome")) else {
            continue;
        };

        let abs = root.join(spec_rel);
        let actual = run_spec(abs.to_str().expect("utf-8 path"));
        checked += 1;

        if outcome.get("Error").is_some() {
            if !matches!(actual, RunOutcome::Error { .. }) {
                failures.push(format!("case '{name}': documented Error, actual {actual}"));
            }
        } else if let Some(complete) = outcome.get("Complete") {
            match &actual {
                RunOutcome::Error { reason } => {
                    failures.push(format!("case '{name}': documented Complete, actual Error: {reason}"));
                }
                RunOutcome::Complete { results } => {
                    if let Some(exp_results) = complete.get("results").and_then(serde_yaml::Value::as_sequence) {
                        for er in exp_results {
                            let rname = er.get("name").and_then(|n| n.as_str()).unwrap_or("");
                            let rstatus = er.get("status").and_then(|s| s.as_str()).unwrap_or("");
                            match results.iter().find(|r| r.name == rname) {
                                Some(r) if r.status.as_str() != rstatus => failures.push(format!(
                                    "case '{name}' / result '{rname}': documented status '{rstatus}', actual '{}'",
                                    r.status.as_str()
                                )),
                                Some(_) => {}
                                None => failures.push(format!("case '{name}': documented result '{rname}' not produced")),
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(checked > 30, "expected to exercise many run_spec cases; only {checked}");
    assert!(
        failures.is_empty(),
        "{} harness-spec outcome mismatches (of {checked} run_spec cases):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
