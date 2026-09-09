//! Harness bootstrap test for specgate.ctsc spec

#[test]
fn harness_validates_spec() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let repo_root = std::path::Path::new(manifest_dir)
        .ancestors()
        .nth(3)
        .expect("failed to derive repository root");
    let spec_path = repo_root.join("specs/specgate.ctsc.spec.yaml");

    let result = specgate::run_spec(spec_path.to_str().unwrap());
    match result {
        specgate::RunOutcome::Complete { results } => {
            for case in &results {
                assert_eq!(case.status, specgate::CaseStatus::Pass, "case '{}' did not pass", case.name);
            }
        }
        specgate::RunOutcome::Error { reason } => panic!("harness error: {reason}"),
    }
}
