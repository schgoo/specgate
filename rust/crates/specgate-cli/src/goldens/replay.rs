#![cfg(any(test, feature = "test-util"))]

// Cohesive implementation group for this subsystem.
use super::*;

/// Verify every replay-enabled matrix row against its declared candidate.
///
/// Returns an error when candidate discovery or replay execution fails.
///
/// # Errors
/// Returns a contextual golden-generation error for operational candidate failures.
pub(super) fn verify_replay(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
) -> Result<(), GoldenError> {
    let root = root.as_ref();
    let out_root = out_root.as_ref();
    let scratch = scratch.as_ref();
    let mut by_binding: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for row in matrix
        .rows
        .iter()
        .filter(|row| row.classification == Classification::ImplementationComponent && row.captures_rust())
    {
        let binding = row
            .rust
            .as_ref()
            .unwrap_or_else(|| panic!("matrix row '{}' capturing Rust must define Rust settings", row.id))
            .binding
            .clone();
        by_binding.entry(binding).or_default().push(row);
    }

    let mut problems = Vec::with_capacity(matrix.rows.len());
    for (binding_key, rows) in by_binding {
        let binding = repo_path(root, &matrix.bindings[&binding_key]);
        let components = rows
            .iter()
            .map(|row| {
                specgate_discovery::identity::ComponentId::from(
                    row.component
                        .as_deref()
                        .unwrap_or_else(|| panic!("matrix row '{}' capturing Rust must name a component", row.id)),
                )
            })
            .collect::<Vec<_>>();
        let candidates = Candidates::discover(binding, None, &components)
            .map_err(|error| GoldenError::message(format!("{binding_key}: candidate discovery failed: {error}")))?;

        let mut planned = Vec::with_capacity(rows.len());
        for row in &rows {
            let plan = candidates.plan(row.rust_dir(out_root));
            match (row.replay.mode, plan) {
                (ReplayMode::Verified, Ok(plan)) => {
                    let candidate = scratch.join("candidates").join(format!("{}.otlp.json", row.id.replace('/', "-")));
                    planned.push((plan, candidate, *row));
                }
                (ReplayMode::LinkOnly, Ok(_plan)) => {}
                (mode @ (ReplayMode::Verified | ReplayMode::LinkOnly), Err(reason)) => {
                    problems.push(format!("{}: matrix marks replay '{mode:?}' but linking failed: {reason}", row.id));
                }
                (ReplayMode::Unsupported, Ok(_plan)) => {
                    problems.push(format!(
                        "{}: matrix marks replay unsupported but the candidate linked successfully; update the matrix",
                        row.id
                    ));
                }
                (ReplayMode::Unsupported, Err(reason)) => {
                    let expected = row
                        .replay
                        .expect_category
                        .as_deref()
                        .unwrap_or_else(|| panic!("unsupported replay row '{}' must declare expectCategory", row.id));
                    match classify(&reason) {
                        Some(actual) if actual.code() == expected => {}
                        Some(actual) => problems.push(format!(
                            "{}: expected replay failure category '{expected}' but got '{}': {reason}",
                            row.id,
                            actual.code()
                        )),
                        None => problems.push(format!(
                            "{}: replay failed for an unrelated, unclassified reason instead of '{expected}': {reason}",
                            row.id
                        )),
                    }
                }
                (mode, _) => problems.push(format!("{}: captured bundles cannot use replay mode '{mode:?}'", row.id)),
            }
        }
        if planned.is_empty() {
            continue;
        }
        let executable = planned
            .iter()
            .map(|(plan, candidate, _row)| (plan.clone(), candidate.clone()))
            .collect::<Vec<_>>();
        let reports = candidates
            .execute(&executable)
            .map_err(|error| GoldenError::message(format!("{binding_key}: batched replay failed: {error}")))?;
        assert_eq!(
            reports.len(),
            planned.len(),
            "{binding_key}: replay execution must return one report per plan (reports={}, plans={})",
            reports.len(),
            planned.len()
        );
        for (_plan, candidate, row) in &planned {
            let bundle = row.rust_dir(out_root);
            let report = compare(bundle.join(trace_file()), candidate, Some(bundle.join(registry_file())), &[]);
            if !report.equivalent {
                problems.push(format!(
                    "{}: candidate replay diverged: failures={:?} mismatches={:?} errors={:?}",
                    row.id, report.validation_failures, report.mismatches, report.errors
                ));
            }
        }
    }
    if !problems.is_empty() {
        return Err(GoldenError::message(format!(
            "replay verification failed:\n  {}",
            problems.join("\n  ")
        )));
    }
    Ok(())
}
// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------
