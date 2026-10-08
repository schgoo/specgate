#![cfg(any(test, feature = "test-util"))]

// Cohesive implementation group for this subsystem.
use super::*;

pub(super) fn expected_artifacts(matrix: &Matrix) -> BTreeSet<RepoPath> {
    let mut expected = BTreeSet::new();
    for row in &matrix.rows {
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            for artifact in &language.artifacts {
                expected
                    .insert(RepoPath::try_from(format!("{}/{name}/{artifact}", row.id)).expect("matrix artifact path must be portable"));
            }
        }
    }
    expected
}

pub(super) fn actual_artifacts(root: impl AsRef<Path>) -> std::io::Result<BTreeSet<RepoPath>> {
    let root = root.as_ref();
    let mut found = BTreeSet::new();
    if !root.is_dir() {
        return Ok(found);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = RepoPath::try_from(relative_display(root, &path)).expect("generated artifact path must be portable");
                if <RepoPath as AsRef<Path>>::as_ref(&relative) != matrix_file() {
                    found.insert(relative);
                }
            }
        }
    }
    Ok(found)
}

pub(super) fn validate_artifacts(matrix: &Matrix, out_root: impl AsRef<Path>) {
    let out_root = out_root.as_ref();
    let mut problems = Vec::new();
    for row in &matrix.rows {
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            let directory = out_root.join(&row.id).join(name);
            for artifact in &language.artifacts {
                let path = directory.join(artifact);
                if !path.is_file() {
                    problems.push(format!("{}/{name}: {artifact} was not generated", row.id));
                }
            }
            if language.expect == Expectation::Failure {
                let path = directory.join(error_file());
                if path.is_file() {
                    let document: serde_json::Value = serde_json::from_slice(&read_bytes(&path)).expect("generated error document");
                    let category = document["category"].as_str().unwrap_or_default();
                    let expected = language.expect_category.as_deref().unwrap_or_default();
                    if category != expected {
                        problems.push(format!(
                            "{}/{name}: expected category '{expected}' but produced '{category}'",
                            row.id
                        ));
                    }
                    if document["outcome"].as_str() != Some("failure") {
                        problems.push(format!("{}/{name}: normalized error is not a failure", row.id));
                    }
                }
                continue;
            }
            if language
                .artifacts
                .iter()
                .any(|artifact| <RepoPath as AsRef<Path>>::as_ref(artifact) == registry_file())
            {
                let registry = directory.join(registry_file());
                let report = validate_registry(&registry, &[]);
                if !report.valid {
                    problems.push(format!("{}/{name}: registry validation failed: {:?}", row.id, report.issues));
                }
                if language
                    .artifacts
                    .iter()
                    .any(|artifact| <RepoPath as AsRef<Path>>::as_ref(artifact) == trace_file())
                {
                    let trace = directory.join(trace_file());
                    let trace_report = validate_trace(&trace);
                    if !trace_report.valid {
                        problems.push(format!("{}/{name}: trace validation failed: {:?}", row.id, trace_report.issues));
                    }
                    if row.linkage_mode() != LinkageMode::Linked {
                        problems.push(format!(
                            "{}/{name}: captured bundles must declare linkage 'linked', found '{:?}'",
                            row.id,
                            row.linkage_mode()
                        ));
                    }
                    let linked = validate_linked(&trace, &registry, &[]);
                    if !linked.valid {
                        problems.push(format!("{}/{name}: linked validation failed: {:?}", row.id, linked.issues));
                    }
                    let bundle = validate_bundle(&directory);
                    if !bundle.valid {
                        problems.push(format!("{}/{name}: bundle validation failed: {:?}", row.id, bundle.issues));
                    }
                }
            }
        }
    }
    assert!(
        problems.is_empty(),
        "generated CTSC artifacts failed native validation:\n  {}",
        problems.join("\n  ")
    );
}

pub(super) fn check_parity(matrix: &Matrix, out_root: impl AsRef<Path>) {
    let out_root = out_root.as_ref();
    let mut problems = Vec::new();
    for row in matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
    {
        let rust = row.rust_dir(out_root).join(registry_file());
        let csharp = row.csharp_dir(out_root).join(registry_file());
        match row.parity.mode {
            ParityMode::ByteIdentical => {
                if !csharp.is_file() {
                    problems.push(format!("{}: parity requires a C# registry", row.id));
                    continue;
                }
                if read_bytes(&rust) != read_bytes(&csharp) {
                    problems.push(format!("{}: Rust and C# registries are not byte-identical", row.id));
                }
            }
            ParityMode::Exception => {
                if !csharp.is_file() {
                    problems.push(format!("{}: a parity exception requires a C# registry", row.id));
                    continue;
                }
                let rust_document: serde_json::Value = serde_json::from_slice(&read_bytes(&rust)).expect("generated Rust registry is JSON");
                let csharp_document: serde_json::Value =
                    serde_json::from_slice(&read_bytes(&csharp)).expect("generated C# registry is JSON");
                let observed = semantic_differences(&rust_document, &csharp_document);
                if observed.is_empty() {
                    problems.push(format!(
                        "{}: parity exception is stale — Rust and C# registries now match; remove the exception",
                        row.id
                    ));
                    continue;
                }
                let observed_set = observed.iter().map(ParityDifference::render).collect::<BTreeSet<_>>();
                let declared_set = row
                    .parity
                    .allowed_differences
                    .iter()
                    .map(ParityDifference::render)
                    .collect::<BTreeSet<_>>();
                for undeclared in observed_set.difference(&declared_set) {
                    problems.push(format!("{}: undeclared cross-language difference {undeclared}", row.id));
                }
                for stale in declared_set.difference(&observed_set) {
                    problems.push(format!("{}: declared cross-language difference no longer occurs {stale}", row.id));
                }
            }
            ParityMode::RustOnly if row.csharp.is_some() || csharp.exists() => {
                problems.push(format!("{}: declared Rust-only but a C# artifact exists", row.id));
            }
            _ => {}
        }
    }
    assert!(problems.is_empty(), "cross-language parity failed:\n  {}", problems.join("\n  "));
}

/// Reject any generated artifact that embeds a machine-specific location.
///
/// Fixture payloads legitimately contain characters that look like Windows
/// paths, so this checks for the concrete roots a leak could come from —
/// the repository, the harness scratch tree, the home directory, and the
/// system temporary directory — rather than guessing from punctuation.
pub(super) fn check_portability(root: impl AsRef<Path>, out_root: impl AsRef<Path>, artifacts: &BTreeSet<RepoPath>) {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let environment = [
        "SPECGATE_CTSC_GOLDENS_SCRATCH",
        "SPECGATE_CACHE_DIR",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
    ]
    .into_iter()
    .filter_map(|variable| std::env::var_os(variable).filter(|value| !value.is_empty()).map(PathBuf::from));
    let roots = [root.to_path_buf(), out_root.to_path_buf(), std::env::temp_dir()]
        .into_iter()
        .chain(environment);
    check_leaks(out_root, artifacts, roots);
}

fn check_leaks(out_root: impl AsRef<Path>, artifacts: &BTreeSet<RepoPath>, roots: impl IntoIterator<Item = PathBuf>) {
    let out_root = out_root.as_ref();
    let mut markers = BTreeSet::new();
    for candidate in roots {
        let display = candidate.to_string_lossy().to_string();
        if display.len() < MARKER_MIN {
            continue;
        }
        markers.insert(display.replace('\\', "/"));
        markers.insert(display);
    }
    markers.insert("file://".to_string());
    markers.insert("ctsc-goldens".to_string());

    let mut problems = Vec::with_capacity(artifacts.len().saturating_mul(markers.len()));
    for artifact in artifacts {
        let path = out_root.join(artifact.as_str().replace('/', std::path::MAIN_SEPARATOR_STR));
        let text = String::from_utf8_lossy(&read_bytes(&path)).to_string();
        for marker in &markers {
            if text.contains(marker.as_str()) {
                problems.push(format!("{artifact}: leaks the machine-specific location '{marker}'"));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "generated artifacts must be location independent:\n  {}",
        problems.join("\n  ")
    );
}
pub(super) fn compare_goldens(matrix: &Matrix, checked_in: impl AsRef<Path>, generated: impl AsRef<Path>) {
    let checked_in = checked_in.as_ref();
    let generated = generated.as_ref();
    let expected = expected_artifacts(matrix);
    let stored = actual_artifacts(checked_in).expect("scan checked-in golden artifacts");
    let mut problems = Vec::with_capacity(expected.len() + stored.len());
    for missing in expected.difference(&stored) {
        problems.push(format!("missing golden: {missing}"));
    }
    for extra in stored.difference(&expected) {
        problems.push(format!("unexpected golden: {extra} (no matrix row declares it)"));
    }
    for artifact in expected.intersection(&stored) {
        let relative = artifact.as_str().replace('/', std::path::MAIN_SEPARATOR_STR);
        if read_bytes(checked_in.join(&relative)) != read_bytes(generated.join(&relative)) {
            problems.push(format!("stale golden: {artifact} differs from freshly generated output"));
        }
    }
    assert!(
        problems.is_empty(),
        "checked-in CTSC goldens are out of date; run `just ctsc-goldens-update`:\n  {}",
        problems.join("\n  ")
    );
}
