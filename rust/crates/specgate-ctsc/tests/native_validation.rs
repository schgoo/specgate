use specgate_ctsc::validation::{validate_linked, validate_registry, validate_trace};
use std::path::{Path, PathBuf};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("docs")
        .join("ctsc")
        .join("corpus")
}

fn files(path: &Path, extensions: &[&str]) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut pending = vec![path.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in std::fs::read_dir(path).expect("corpus directory") {
            let entry = entry.expect("corpus entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| extensions.contains(&value))
            {
                result.push(path);
            }
        }
    }
    result.sort();
    result
}

#[test]
fn native_registry_validator_classifies_entire_corpus() {
    let root = corpus().join("registry");
    for path in files(&root.join("valid"), &["json"]) {
        let report = validate_registry(&path, &[]);
        assert!(report.valid, "{}: {:#?}", path.display(), report.issues);
    }
    for path in files(&root.join("invalid"), &["json"]) {
        let report = validate_registry(&path, &[]);
        assert!(!report.valid, "{} unexpectedly passed", path.display());
    }
}

#[test]
fn native_trace_validator_classifies_entire_corpus_including_jsonl() {
    let root = corpus().join("trace");
    for path in files(&root.join("valid"), &["json", "jsonl"]) {
        let report = validate_trace(&path);
        assert!(report.valid, "{}: {:#?}", path.display(), report.issues);
    }
    for path in files(&root.join("invalid"), &["json", "jsonl"]) {
        let report = validate_trace(&path);
        assert!(!report.valid, "{} unexpectedly passed", path.display());
    }
}

#[test]
fn native_linked_validator_classifies_entire_corpus() {
    let root = corpus().join("linked");
    for path in files(&root.join("valid"), &["json"]) {
        if !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("trace"))
        {
            continue;
        }
        let registry = path.parent().expect("fixture directory").join("registry.json");
        let report = validate_linked(&path, &registry, &[]);
        assert!(report.valid, "{}: {:#?}", path.display(), report.issues);
    }
    for path in files(&root.join("invalid"), &["json"]) {
        if !path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("trace"))
        {
            continue;
        }
        let registry = std::fs::read_dir(path.parent().expect("fixture directory"))
            .expect("fixture directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate.extension().and_then(|value| value.to_str()) == Some("json")
                    && !candidate
                        .file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(|name| name.starts_with("trace"))
            })
            .expect("registry beside trace");
        let report = validate_linked(&path, &registry, &[]);
        assert!(!report.valid, "{} unexpectedly passed", path.display());
    }
}
