#![cfg(any(test, feature = "test-util"))]

//! Matrix source discovery, coverage, and replacement-reference validation.
//!
//! Every discovered fixture must be covered exactly by a matrix row or an
//! explicit exclusion, and replacement rows must reference existing sources.
use super::*;

/// Check whether a matrix identifier uses stable lowercase kebab-case.
pub(super) fn is_stable(value: impl AsRef<str>) -> bool {
    let value = value.as_ref();
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !value.starts_with('-')
        && !value.ends_with('-')
        && !value.contains("--")
}

/// Check whether a path names a Rust or C# source, including `.in` templates.
pub(super) fn is_source(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
    let stem = name.strip_suffix(".in").unwrap_or(name);
    Path::new(stem)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "rs" | "cs"))
}

/// Recursively collect deterministic repository-relative source paths.
///
/// Returns a contextual error when a source directory or entry cannot be read.
pub(super) fn collect_sources(root: impl AsRef<Path>, relative_root: impl AsRef<Path>) -> Result<Vec<PathBuf>, GoldenError> {
    let root = root.as_ref();
    let relative_root = relative_root.as_ref();
    let mut found = Vec::new();
    let start = repo_path(root, relative_root);
    let mut stack = vec![start];
    while let Some(directory) = stack.pop() {
        let entries =
            std::fs::read_dir(&directory).map_err(|error| GoldenError::wrap(format!("failed to list {}", directory.display()), error))?;
        for entry in entries {
            let entry =
                entry.map_err(|error| GoldenError::wrap(format!("failed to inspect an entry in {}", directory.display()), error))?;
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if ["bin", "obj", "target"].iter().any(|ignored| name == std::ffi::OsStr::new(ignored)) {
                    continue;
                }
                stack.push(path);
            } else if is_source(&path) {
                found.push(
                    path.strip_prefix(root)
                        .expect("walked source remains under repository root")
                        .to_path_buf(),
                );
            }
        }
    }
    found.sort();
    Ok(found)
}

/// Render a path relative to the repository with portable separators.
///
/// Panics when `path` is outside `root`, which violates the source-walk contract.
pub(super) fn relative_display(root: impl AsRef<Path>, path: impl AsRef<Path>) -> String {
    let root = root.as_ref();
    let path = path.as_ref();
    path.strip_prefix(root)
        .unwrap_or_else(|_error| panic!("{} is outside the repository", path.display()))
        .to_string_lossy()
        .replace('\\', "/")
}

/// Detect source text containing a Rust or C# operation annotation.
pub(super) fn declares_operation(text: impl AsRef<str>) -> bool {
    let text = text.as_ref();
    text.contains("spec_operation(") || text.contains("[SpecOperation(")
}

/// Detect any supported `SpecGate` annotation in source text.
pub(super) fn has_annotation(text: impl AsRef<str>) -> bool {
    let text = text.as_ref();
    declares_operation(text)
        || [
            "spec_setup(",
            "spec_component!(",
            "SpecEvent",
            "[SpecSetup(",
            "[SpecEvent(",
            "[SpecException(",
            "[SpecInput(",
        ]
        .iter()
        .any(|token| text.contains(token))
}

/// Validate exact source coverage and declared exclusions for the golden matrix.
///
/// Returns one aggregated diagnostic for missing, stale, or contradictory coverage.
pub(super) fn check_coverage(root: impl AsRef<Path>, matrix: &Matrix) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let covered = matrix
        .rows
        .iter()
        .flat_map(|row| row.sources.iter().map(|source| (source.clone(), row.id.clone())))
        .collect::<Vec<_>>();
    let mut covered_paths = BTreeMap::new();
    for (source, row_id) in covered {
        covered_paths.entry(source).or_insert_with(Vec::new).push(row_id);
    }
    let excluded = matrix
        .sources
        .exclusions
        .iter()
        .flat_map(|exclusion| exclusion.paths.iter().cloned())
        .collect::<BTreeSet<_>>();
    let mut problems = Vec::new();
    for exclusion in &matrix.sources.exclusions {
        if exclusion.rule.is_empty() || exclusion.description.is_empty() {
            problems.push("every source exclusion must state its rule and why it applies".to_string());
        }
    }
    if !matrix
        .sources
        .exclusions
        .iter()
        .any(|exclusion| exclusion.rule == "no-spec-annotation")
    {
        problems.push("the matrix must encode the rule that unannotated sources are out of scope".to_string());
    }

    let mut universe = BTreeSet::new();
    for source_root in &matrix.sources.roots {
        universe.extend(
            collect_sources(root, source_root)?
                .into_iter()
                .map(|path| path.to_string_lossy().replace('\\', "/")),
        );
    }

    for source in &universe {
        let path = repo_path(root, source);
        let text =
            std::fs::read_to_string(&path).map_err(|error| GoldenError::wrap(format!("failed to read {}", path.display()), error))?;
        let annotated = declares_operation(&text);
        let covered = covered_paths.contains_key(source.as_str());
        if annotated && !covered {
            problems.push(format!("{source}: declares operations but no matrix row lists it as a source"));
        }
        if !annotated && covered {
            problems.push(format!("{source}: is listed as a matrix source but declares no operation"));
        }
        if !annotated && !covered && !excluded.contains(source.as_str()) && has_annotation(&text) {
            problems.push(format!(
                "{source}: carries spec annotations but is neither covered by a row nor an explicit matrix exclusion"
            ));
        }
    }
    for source in covered_paths.keys() {
        if !universe.contains(source.as_str()) {
            problems.push(format!("{source}: matrix source does not exist under any declared source root"));
        }
    }
    for source in &excluded {
        if !universe.contains(source.as_str()) {
            problems.push(format!("{source}: matrix exclusion does not exist under any declared source root"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(GoldenError::message(format!(
            "matrix source coverage is incomplete:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// Validate that replacement rows reference existing source files.
///
/// Returns an aggregated diagnostic for missing replacement references.
pub(super) fn check_replacements(root: impl AsRef<Path>, matrix: &Matrix) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let mut problems = Vec::new();
    for row in matrix.rows.iter().filter(|row| row.classification == Classification::Replacement) {
        let replacement = row
            .replacement
            .as_ref()
            .unwrap_or_else(|| panic!("validated replacement row '{}' must contain replacement metadata", row.id));
        for reference in &replacement.references {
            let path = repo_path(root, &reference.path);
            if !path.is_file() {
                problems.push(format!("{}: {} does not exist", row.id, reference.path));
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|error| GoldenError::message(format!("failed to read {}: {error}", path.display())))?;
            let declared = text.contains(&format!("fn {}(", reference.test)) || text.contains(&format!("void {}(", reference.test));
            if !declared {
                problems.push(format!("{}: {} does not declare test '{}'", row.id, reference.path, reference.test));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(GoldenError::message(format!(
            "replacement rows must point at existing CTSC-native tests:\n  {}",
            problems.join("\n  ")
        )))
    }
}
// ---------------------------------------------------------------------------
// Artifact generation
// ---------------------------------------------------------------------------
