use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .to_path_buf()
}

fn specgate() -> Command {
    Command::new(env!("CARGO_BIN_EXE_specgate"))
}

fn scratch_file(name: &str, value: &serde_json::Value) -> PathBuf {
    let id = SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    let path = repo_root()
        .join("rust")
        .join("target")
        .join(format!("cli-{name}-{}-{id}.json", std::process::id()));
    std::fs::write(&path, serde_json::to_vec(value).expect("serialize")).expect("write scratch");
    path
}

#[test]
fn validate_commands_use_documented_exit_codes() {
    let corpus = repo_root().join("docs").join("ctsc").join("corpus");
    let valid = corpus.join("trace").join("valid").join("sequential.otlp.json");
    let invalid = corpus.join("trace").join("invalid").join("wrong-version.otlp.json");

    assert!(specgate().args(["validate", "trace"]).arg(&valid).status().unwrap().success());
    assert_eq!(
        specgate().args(["validate", "trace"]).arg(&invalid).status().unwrap().code(),
        Some(1)
    );
    assert_eq!(specgate().args(["validate", "trace"]).status().unwrap().code(), Some(2));
}

#[test]
fn compare_reports_equivalence_and_stable_mismatch_paths() {
    let reference = repo_root()
        .join("docs")
        .join("ctsc")
        .join("corpus")
        .join("trace")
        .join("valid")
        .join("sequential.otlp.json");
    assert!(
        specgate()
            .arg("compare")
            .arg(&reference)
            .arg(&reference)
            .status()
            .unwrap()
            .success()
    );

    let mut changed: serde_json::Value = serde_json::from_slice(&std::fs::read(&reference).unwrap()).unwrap();
    let operation = changed["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|span| span["name"] == "conformance.operation")
        .unwrap();
    let result = operation["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|event| event["name"] == "conformance.result")
        .unwrap();
    let value = result["attributes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|attribute| attribute["key"] == "conformance.result.value")
        .unwrap();
    value["value"] = serde_json::json!({"stringValue":"changed"});
    let changed = scratch_file("changed", &changed);
    let output = specgate().arg("compare").arg(&reference).arg(&changed).output().unwrap();
    std::fs::remove_file(changed).unwrap();
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("policy: ctsc.strict/0.1.0"), "{stdout}");
    assert!(stdout.contains("mismatch scenario[\""), "{stdout}");
    assert!(stdout.contains("expected"), "{stdout}");
    assert!(stdout.contains("actual"), "{stdout}");
}

#[test]
fn explicit_registry_imports_resolve_by_registry_id() {
    let imports = repo_root()
        .join("docs")
        .join("ctsc")
        .join("corpus")
        .join("registry")
        .join("valid")
        .join("imports");
    let root_path = imports.join("orders.registry.json");
    let imported = imports.join("tax.registry.json");
    let mut root: serde_json::Value = serde_json::from_slice(&std::fs::read(&root_path).unwrap()).unwrap();
    root["imports"][0].as_object_mut().unwrap().remove("uri");
    let root = scratch_file("explicit-import", &root);
    let output = specgate()
        .args(["validate", "registry"])
        .arg(&root)
        .args(["--import"])
        .arg(&imported)
        .output()
        .unwrap();
    std::fs::remove_file(root).unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
}
