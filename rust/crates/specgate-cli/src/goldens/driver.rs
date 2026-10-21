#![cfg(any(test, feature = "test-util"))]

// Cohesive implementation group for this subsystem.
use super::*;

pub(super) fn clear_artifacts(matrix: &Matrix, root: &Path) -> Result<(), GoldenError> {
    preflight_artifacts(matrix, root, false)?;
    for artifact in expected_artifacts(matrix) {
        let path = root.join(artifact);
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|error| GoldenError::wrap(format!("failed to replace {}", path.display()), error))?;
        }
    }
    Ok(())
}

pub(super) fn run(mode: Mode) -> Result<(), GoldenError> {
    let current = std::env::current_dir().map_err(|source| GoldenError::wrap("failed to read current directory", source))?;
    let root = repo_root(current);
    let matrix = load_matrix(&root);
    check_shape(&matrix);
    check_coverage(&root, &matrix)?;
    check_replacements(&root, &matrix)?;

    let checked_in = repo_path(&root, &matrix.artifact_root);
    preflight_artifacts(&matrix, &checked_in, mode == Mode::Check)?;
    let scratch = std::env::var_os("SPECGATE_CTSC_GOLDENS_SCRATCH")
        .map_or_else(|| repo_path(&root, "rust/target").join("ctsc-goldens"), PathBuf::from);
    let generated = match mode {
        Mode::Update => checked_in.clone(),
        Mode::Check => scratch.join("generated"),
    };
    clear_artifacts(&matrix, &generated)?;
    std::fs::create_dir_all(&generated).map_err(|source| GoldenError::wrap("failed to create generated artifact directory", source))?;

    let rust_groups = rust_groups(&root, &matrix)?;
    for rows in rust_groups.values() {
        generate_rust(&root, &matrix, &generated, &scratch, rows)?;
    }
    generate_csharp(&root, &matrix, &generated, &scratch)?;
    generate_negatives(&root, &matrix, &generated, &scratch)?;

    validate_artifacts(&matrix, &generated);
    let canonical_root = if mode == Mode::Check { &checked_in } else { &generated };
    check_parity(&matrix, &generated, &scratch, canonical_root);
    let produced = actual_artifacts(&generated).map_err(|source| GoldenError::wrap("failed to enumerate generated artifacts", source))?;
    check_portability(&root, &generated, &produced);

    let expected = expected_artifacts(&matrix);
    let mut problems = Vec::with_capacity(expected.len().saturating_add(produced.len()));
    for missing in expected.difference(&produced) {
        problems.push(format!("declared artifact was never generated: {missing}"));
    }
    for extra in produced.difference(&expected) {
        problems.push(format!("generated artifact is not declared by any matrix row: {extra}"));
    }
    assert!(
        problems.is_empty(),
        "the matrix and generated artifacts disagree:\n  {}",
        problems.join("\n  ")
    );

    verify_replay(&root, &matrix, &generated, &scratch)?;

    if mode == Mode::Check {
        compare_goldens(&matrix, &checked_in, &generated);
    }

    let components = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
        .count();
    let negatives = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::NegativeFixture)
        .count();
    let replacements = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::Replacement)
        .count();
    println!(
        "ctsc goldens {}: {components} component rows, {negatives} negative rows, {replacements} replacement rows, {} artifacts",
        match mode {
            Mode::Update => "updated",
            Mode::Check => "checked",
        },
        produced.len()
    );
    Ok(())
}
