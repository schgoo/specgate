#![cfg(any(test, feature = "test-util"))]

// Cohesive implementation group for this subsystem.
use super::*;

pub(super) fn expected_artifacts(matrix: &Matrix) -> BTreeSet<RepoPath> {
    let mut expected = BTreeSet::new();
    for row in &matrix.rows {
        if let Some(reference) = &row.reference {
            for artifact in &reference.artifacts {
                expected.insert(RepoPath::try_from(format!("{}/{artifact}", row.id)).expect("portable reference artifact"));
            }
        }
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            for artifact in language.error_artifacts() {
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
        if row.captures_any() {
            problems.extend(
                capture_problems(&row.reference_dir(out_root))
                    .into_iter()
                    .map(|problem| format!("{}/reference: {problem}", row.id)),
            );
        } else if row.reference.is_some() {
            let report = validate_registry(row.reference_dir(out_root).join(registry_file()), &[]);
            if !report.valid {
                problems.push(format!("{}/reference: registry validation failed: {:?}", row.id, report.issues));
            }
        }
        for (language, name) in [(row.rust.as_ref(), "rust"), (row.csharp.as_ref(), "csharp")] {
            let Some(language) = language else { continue };
            let directory = out_root.join(&row.id).join(name);
            for artifact in language.error_artifacts() {
                let path = directory.join(artifact);
                if !path.is_file() {
                    problems.push(format!("{}/{name}: {artifact} was not generated", row.id));
                }
            }
            if !language.succeeds() {
                let path = directory.join(error_file());
                if path.is_file() {
                    let document: serde_json::Value = serde_json::from_slice(&read_bytes(&path)).expect("generated error document");
                    let category = document["category"].as_str().unwrap_or_default();
                    let expected = language.failure_category().unwrap_or_default();
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
            }
        }
    }
    assert!(
        problems.is_empty(),
        "generated CTSC artifacts failed native validation:\n  {}",
        problems.join("\n  ")
    );
}

/// The same native validation for selected and transient producer bundles.
pub(super) fn capture_problems(directory: &Path) -> Vec<String> {
    let registry = directory.join(registry_file());
    let trace = directory.join(trace_file());
    let mut problems = Vec::new();
    for (kind, report) in [
        ("registry", validate_registry(&registry, &[])),
        ("trace", validate_trace(&trace)),
        ("linked", validate_linked(&trace, &registry, &[])),
        ("bundle", validate_bundle(directory)),
    ] {
        if !report.valid {
            problems.push(format!("{kind} validation failed: {:?}", report.issues));
        }
    }
    problems
}

/// Anchor the selected full native registry to the checked-in canonical bytes
/// before comparing independent producers to that same oracle. In update mode
/// `canonical_root` is the newly generated shared tree.
pub(super) fn check_parity(matrix: &Matrix, out_root: &Path, scratch: &Path, canonical_root: &Path) {
    let mut problems = Vec::new();
    for row in matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
    {
        let reference = row.reference.as_ref().expect("component registry oracle");
        let canonical = read_bytes(row.reference_dir(canonical_root).join(registry_file()));
        for (directory, implementation) in row.implementations() {
            let path = row.producer_registry(out_root, scratch, &implementation.binding);
            let report = validate_registry(&path, &[]);
            if !report.valid {
                problems.push(format!(
                    "{} [{directory}]: producer registry validation failed: {:?}",
                    row.id, report.issues
                ));
            }
            let producer = read_bytes(path);
            problems.extend(registry_parity_problems(row, &implementation.binding, &producer, &canonical));
        }
        if matches!(row.parity.mode, ParityMode::ByteIdentical | ParityMode::Exception) && (row.rust.is_none() || row.csharp.is_none()) {
            problems.push(format!("{}: registry parity requires both producers", row.id));
        }
        if row.parity.mode == ParityMode::RustOnly
            && (row.csharp.is_some() || reference.implementation != row.rust.as_ref().expect("Rust-only endpoint").binding)
        {
            problems.push(format!("{}: declared Rust-only but selects or declares another producer", row.id));
        }
    }
    assert!(problems.is_empty(), "cross-language parity failed:\n  {}", problems.join("\n  "));
}

/// Reuse the exact semantic parity model, retaining language-side orientation
/// independently of endpoint identity or canonical producer selection.
pub(super) fn registry_parity_problems(row: &Row, label: &str, producer: &[u8], canonical: &[u8]) -> Vec<String> {
    let reference = row.reference.as_ref().expect("component registry oracle");
    if label == reference.implementation {
        return if producer == canonical {
            Vec::new()
        } else {
            vec![format!(
                "{} [{label}]: selected native registry differs from canonical golden",
                row.id
            )]
        };
    }
    if row.parity.mode != ParityMode::Exception {
        return if producer == canonical {
            Vec::new()
        } else {
            vec![format!(
                "{} [{label}]: producer registry is not byte-identical to canonical golden",
                row.id
            )]
        };
    }
    let (directory, _) = row.implementation(label).expect("declared producer");
    let (rust, csharp) = if directory == "rust" {
        (producer, canonical)
    } else {
        (canonical, producer)
    };
    let rust = serde_json::from_slice(rust).expect("native Rust registry JSON");
    let csharp = serde_json::from_slice(csharp).expect("native C# registry JSON");
    let observed = semantic_differences(&rust, &csharp);
    if observed.is_empty() {
        return vec![format!(
            "{}: parity exception is stale — registries now match; remove the exception",
            row.id
        )];
    }
    let observed = observed.iter().map(ParityDifference::render).collect::<BTreeSet<_>>();
    let declared = row
        .parity
        .allowed_differences
        .iter()
        .map(ParityDifference::render)
        .collect::<BTreeSet<_>>();
    observed
        .difference(&declared)
        .map(|difference| format!("{}: undeclared cross-language difference {difference}", row.id))
        .chain(
            declared
                .difference(&observed)
                .map(|difference| format!("{}: declared cross-language difference no longer occurs {difference}", row.id)),
        )
        .collect()
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
