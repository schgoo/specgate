#![cfg(any(test, feature = "test-util"))]

//! Directed implementation checks using the existing replay planner and strict comparator.
use super::*;

struct ReplayGroup<'a> {
    implementation: specgate_discovery::binding::Implementation,
    label: String,
    checks: Vec<(&'a Row, &'a ReplayCheck)>,
}

fn replay_groups<'a>(binding: &Path, matrix: &'a Matrix) -> Result<Vec<ReplayGroup<'a>>, GoldenError> {
    let mut groups = Vec::<ReplayGroup<'a>>::new();
    for row in &matrix.rows {
        for check in &row.checks {
            let component = row.component.as_deref().expect("validated check component");
            let resolved = resolve_implementation(binding, component, Some(&check.candidate))
                .map_err(|error| GoldenError::message(format!("{}: candidate '{}' is unavailable: {error}", row.id, check.candidate)))?;
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.implementation == resolved.implementation && group.label == check.candidate)
            {
                group.checks.push((row, check));
            } else {
                groups.push(ReplayGroup {
                    implementation: resolved.implementation,
                    label: check.candidate.clone(),
                    checks: vec![(row, check)],
                });
            }
        }
    }
    Ok(groups)
}

/// Verify each shared reference capture against its declared candidate.
/// Batch only implementations with identical source, test, and execution settings.
pub(super) fn verify_replay(
    root: impl AsRef<Path>,
    matrix: &Matrix,
    out_root: impl AsRef<Path>,
    scratch: impl AsRef<Path>,
) -> Result<(), GoldenError> {
    let binding = repo_path(root, &matrix.binding);
    let out_root = out_root.as_ref();
    let scratch = scratch.as_ref();
    let mut problems = Vec::new();
    for (group_index, group) in replay_groups(&binding, matrix)?.into_iter().enumerate() {
        let ReplayGroup {
            implementation,
            label,
            checks,
        } = group;
        let components = checks
            .iter()
            .map(|(row, _)| specgate_discovery::identity::ComponentId::from(row.component.as_deref().expect("validated component")))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let label = specgate_discovery::identity::ImplementationLabel::from(label);
        let candidates = match Candidates::discover(&binding, Some(&label), &components) {
            Ok(candidates) => candidates,
            Err(error) => {
                for (row, check) in &checks {
                    let context = format!("{} [{} -> {}]", row.id, check.reference, check.candidate);
                    if check.mode == ReplayMode::Unsupported {
                        if let Some(problem) = unsupported_problem(
                            &context,
                            check.expect_category.as_deref().expect("validated category"),
                            &error.to_string(),
                        ) {
                            problems.push(problem);
                        }
                    } else {
                        problems.push(format!("{context}: candidate discovery failed: {error}"));
                    }
                }
                continue;
            }
        };
        let mut planned = Vec::new();
        for (index, (row, check)) in checks.iter().enumerate() {
            let context = format!("{} [{} -> {}]", row.id, check.reference, check.candidate);
            let bundle = row.reference_dir(out_root);
            match (check.mode, candidates.plan(&bundle)) {
                (ReplayMode::Verified, Ok(plan)) => {
                    let candidate = scratch.join("candidates").join(format!("{group_index}-{index}.otlp.json"));
                    planned.push((plan, candidate, bundle, context));
                }
                // Explicit link-only checks do not assert behavioral equivalence.
                (ReplayMode::LinkOnly, Ok(_)) => {}
                (mode @ (ReplayMode::Verified | ReplayMode::LinkOnly), Err(reason)) => {
                    problems.push(format!("{context}: matrix marks replay '{mode:?}' but linking failed: {reason}"));
                }
                (ReplayMode::Unsupported, Ok(_)) => {
                    problems.push(format!(
                        "{context}: unsupported expectation is stale; candidate linked successfully"
                    ));
                }
                (ReplayMode::Unsupported, Err(reason)) => {
                    let expected = check.expect_category.as_deref().expect("validated unsupported category");
                    if let Some(problem) = unsupported_problem(&context, expected, &reason.to_string()) {
                        problems.push(problem);
                    }
                }
            }
        }
        if planned.is_empty() {
            continue;
        }
        let executable = planned
            .iter()
            .map(|(plan, candidate, _, _)| (plan.clone(), candidate.clone()))
            .collect::<Vec<_>>();
        let reports = candidates
            .execute(&executable)
            .map_err(|error| GoldenError::message(format!("{}: batched replay failed: {error}", implementation.sources.display())))?;
        assert_eq!(reports.len(), planned.len(), "one replay report per plan");
        for (_, candidate, bundle, context) in &planned {
            let report = compare(bundle.join(trace_file()), candidate, Some(bundle.join(registry_file())), &[]);
            if !report.equivalent {
                problems.push(format!(
                    "{context}: candidate replay diverged: failures={:?} mismatches={:?} errors={:?}",
                    report.validation_failures, report.mismatches, report.errors
                ));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(GoldenError::message(format!(
            "replay verification failed:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// Accept only the exact declared planner category, never an unrelated failure.
pub(super) fn unsupported_problem(context: &str, expected: &str, reason: &str) -> Option<String> {
    match classify(reason) {
        Some(actual) if actual.code() == expected => None,
        Some(actual) => Some(format!(
            "{context}: expected replay failure category '{expected}' but got '{}': {reason}",
            actual.code()
        )),
        None => Some(format!(
            "{context}: replay failed for an unrelated, unclassified reason instead of '{expected}': {reason}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_groups_preserve_component_executor_selection() {
        use specgate_discovery::binding::{LanguageOptions, Runtime};

        let root = repo_root(std::env::current_dir().unwrap());
        let matrix = load_matrix(&root);
        let binding = repo_path(&root, &matrix.binding);
        let groups = replay_groups(&binding, &matrix).unwrap();
        let group_index = |component: &str| {
            groups
                .iter()
                .position(|group| group.checks.iter().any(|(row, _)| row.component.as_deref() == Some(component)))
                .expect("fixture has a replay check")
        };
        let smol = group_index("fixture.async_smol_timer");
        let tokio = group_index("fixture.async_tokio_timer");
        assert_ne!(smol, tokio);
        assert_eq!(group_index("fixture.async_fetch"), smol);
        assert_eq!(group_index("fixture.fallible_unit"), smol);
        for (index, runtime) in [(smol, Runtime::Smol), (tokio, Runtime::Tokio)] {
            assert_eq!(
                groups[index].implementation.options,
                LanguageOptions::Rust { runtime, executor: None }
            );
        }
        for group in groups {
            for (row, check) in group.checks {
                let resolved = resolve_implementation(&binding, row.component.as_deref().unwrap(), Some(&check.candidate)).unwrap();
                assert_eq!(group.implementation, resolved.implementation);
            }
        }
    }
}
