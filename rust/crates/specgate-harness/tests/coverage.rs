//! Coverage measurement: running a spec through the harness with coverage
//! enabled instruments the implementation crate and reports what fraction of
//! its lines the spec cases exercised. Slow (builds an instrumented runner and
//! shells `cargo run` + `llvm-cov`) and depends on the `llvm-tools` component,
//! so it is `#[ignore]`d; invoke with `--ignored` (via `just coverage`).

use specgate_harness::{CaseStatus, CoverageOutcome, run_spec_with_coverage};

fn repo_root() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p.pop(); // rust
    p
}

#[test]
#[ignore = "instrumented coverage run is slow and needs llvm-tools; invoke with --ignored"]
fn partial_coverage_is_measured() {
    let spec = repo_root().join("test/rust/crates/specgate-fixtures/specs/coverage_partial.spec.yaml");
    match run_spec_with_coverage(spec.to_str().expect("utf-8 path")) {
        CoverageOutcome::Error { reason } => panic!("coverage run errored: {reason}"),
        CoverageOutcome::Unavailable { reason, .. } => {
            // Acceptable where the coverage toolchain is absent (e.g. CI without
            // llvm-tools). Don't fail — the mechanism is exercised where tools
            // are present (locally / via `just coverage`).
            eprintln!("coverage unavailable (skipping bound checks): {reason}");
        }
        CoverageOutcome::Measured { results, coverage } => {
            // All cases still run and pass under instrumentation.
            assert!(!results.is_empty(), "no case results");
            assert!(
                results.iter().all(|r| r.status == CaseStatus::Pass),
                "cases should pass under coverage: {:?}",
                results.iter().map(|r| (&r.name, r.status)).collect::<Vec<_>>()
            );

            // The fixture exercises one branch of `classify` and never calls
            // `never_called`, so coverage must be strictly partial.
            assert!(coverage.lines_total > 0, "no lines counted");
            assert!(coverage.lines_covered > 0, "expected some covered lines");
            assert!(
                coverage.lines_covered < coverage.lines_total,
                "expected partial coverage, got {}/{} (100%)",
                coverage.lines_covered,
                coverage.lines_total
            );
            assert!(
                coverage.percent > 0.0 && coverage.percent < 100.0,
                "percent should be strictly between 0 and 100, got {}",
                coverage.percent
            );

            // The implementation source must appear in the per-file breakdown.
            assert!(
                coverage
                    .files
                    .iter()
                    .any(|f| f.path.replace('\\', "/").contains("specgate-coverage-fixture")),
                "coverage-fixture source missing from per-file coverage: {:?}",
                coverage.files.iter().map(|f| &f.path).collect::<Vec<_>>()
            );
        }
    }
}
