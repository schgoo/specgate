#![cfg(any(test, feature = "test-util"))]

// Cohesive implementation group for this subsystem.
use super::*;

pub(super) fn clear_artifacts(root: impl AsRef<Path>) -> std::io::Result<()> {
    let root = root.as_ref();
    if !root.is_dir() {
        return Ok(());
    }
    let entries = std::fs::read_dir(root)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else if entry.file_name() != matrix_file() {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

pub(super) fn run(mode: Mode) -> Result<(), GoldenError> {
    let root = repo_root();
    let matrix = load_matrix(&root);
    check_shape(&matrix);
    check_coverage(&root, &matrix)?;
    check_replacements(&root, &matrix)?;

    let checked_in = repo_path(&root, &matrix.artifact_root);
    let scratch = std::env::var_os("SPECGATE_CTSC_GOLDENS_SCRATCH")
        .map_or_else(|| repo_path(&root, "rust/target").join("ctsc-goldens"), PathBuf::from);
    let generated = match mode {
        Mode::Update => checked_in.clone(),
        Mode::Check => scratch.join("generated"),
    };
    clear_artifacts(&generated).map_err(|source| GoldenError::wrap("failed to clear generated artifacts", source))?;
    std::fs::create_dir_all(&generated).map_err(|source| GoldenError::wrap("failed to create generated artifact directory", source))?;

    let binding_keys = matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent)
        .filter_map(|row| row.rust.as_ref().map(|language| language.binding.clone()))
        .collect::<BTreeSet<_>>();
    for binding_key in &binding_keys {
        generate_rust(&root, &matrix, &generated, binding_key)?;
    }
    generate_csharp(&root, &matrix, &generated)?;
    generate_negatives(&root, &matrix, &generated, &scratch)?;

    validate_artifacts(&matrix, &generated);
    check_parity(&matrix, &generated);
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
