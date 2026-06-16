use std::path::{Path, PathBuf};
use std::sync::Arc;

use specgate_harness::Harness;
use specgate_rust_backend::RustBackend;
use specgate_types::RunOutcome;

#[test]
fn rust_annotations_spec_passes_through_harness() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root should exist")
        .parent()
        .expect("repo root should exist")
        .to_path_buf();
    let _cleanup = GeneratedTestCleanup::new(&repo_root);
    let mut harness = Harness::new(&repo_root);
    harness.register_backend("rust".to_string(), Arc::new(RustBackend::default()));

    let outcome = harness.run_spec("rust.annotations.spec.yaml");
    match outcome {
        RunOutcome::Complete { report } => {
            assert_eq!(report.failed, 0, "all harness cases should pass");
            assert_eq!(report.passed, report.total, "all harness cases should pass");
            assert_eq!(report.total, 50, "expected every rust.annotations case");
        }
        RunOutcome::Error { error } => panic!("harness run failed: {error:?}"),
    }
}

struct GeneratedTestCleanup {
    generated_test_path: PathBuf,
}

impl GeneratedTestCleanup {
    fn new(repo_root: &Path) -> Self {
        let generated_test_path = repo_root
            .join("rust")
            .join("crates")
            .join("specgate-annotations")
            .join("tests")
            .join("specgate_generated.rs");
        // skip cleanup
        Self {
            generated_test_path,
        }
    }
}

impl Drop for GeneratedTestCleanup {
    fn drop(&mut self) {
        // skip cleanup
    }
}
