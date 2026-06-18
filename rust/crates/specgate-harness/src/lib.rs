//! SpecGate harness — entry point.
//!
//! `run_spec(path)` loads a spec file, resolves its binding, parses the
//! corresponding fixture source file with `syn`, and symbolically
//! interprets each case to produce a trace stream. It then
//! subsequence-matches the case's `expected:` list against the trace
//! stream and reports per-case `pass` / `fail` along with the full
//! actual trace.
//!
//! Why interpretation rather than compile + run?  The fixtures use
//! `#[spec_event]` directly on bare struct fields. Procedural attribute
//! macros are not permitted on field positions in stable Rust, so the
//! fixture sources cannot be compiled by `rustc` as-is.  Interpreting
//! the source — which `syn` happily parses — sidesteps that limitation
//! while keeping the harness fully in-process.

mod binding;
mod discover;
mod interpret;
mod match_traces;
mod spec;
mod types;

pub use types::{CaseResult, CaseStatus, RunOutcome, TraceEvent};

use std::path::{Path, PathBuf};

/// Run the spec at `spec_path` (relative to the current working
/// directory or absolute).
///
/// Returns `RunOutcome::Complete { results }` if loading + dispatch
/// succeeded, `RunOutcome::Error { reason }` for any pre-execution
/// failure (bad YAML, missing binding, missing setup/operation,
/// uncompilable fixture source, no test cases).
pub fn run_spec(spec_path: &str) -> RunOutcome {
    let path = PathBuf::from(spec_path);
    let raw = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            return RunOutcome::Error {
                reason: format!("spec file not found: {spec_path}"),
            };
        }
    };

    // Try to load.  Distinguishes "not valid YAML" from "shape wrong".
    let _value: serde_yaml::Value = match serde_yaml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            return RunOutcome::Error {
                reason: "spec file is not valid YAML".into(),
            };
        }
    };

    let parsed = match spec::load_spec(&path) {
        Ok(s) => s,
        Err(spec::ParseError::Yaml(_)) => {
            return RunOutcome::Error {
                reason: "spec file is not valid YAML".into(),
            };
        }
        Err(spec::ParseError::Io(_)) => {
            return RunOutcome::Error {
                reason: format!("spec file not found: {spec_path}"),
            };
        }
        Err(spec::ParseError::Shape(s)) => {
            return RunOutcome::Error {
                reason: format!("spec shape error: {s}"),
            };
        }
    };

    if parsed.cases.is_empty() {
        return RunOutcome::Error {
            reason: "spec has no test cases".into(),
        };
    }

    // Resolve binding.
    let binding_path = match parsed.binding_path.as_deref() {
        Some(p) => p,
        None => {
            return RunOutcome::Error {
                reason: "spec has no binding".into(),
            };
        }
    };
    let binding_full = spec::binding_path_resolved(&path, binding_path);
    let binding = match binding::load_binding(&binding_full) {
        Some(b) => b,
        None => {
            return RunOutcome::Error {
                reason: format!("binding '{binding_path}' not found"),
            };
        }
    };

    // Discover fixture source by searching the package src/ directory for
    // the file whose annotations match the spec's required operations and
    // setups. We scan every `.rs` under `src/`. If we cannot match by name
    // we fall back to a basename-derived guess.
    let src_dir = binding.package_root.join("src");
    let mut candidate_sources: Vec<(PathBuf, String)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&src_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    candidate_sources.push((p, s));
                }
            }
        }
    }

    // Build the set of required (setup_fn, operation) names across all cases.
    let mut required_setups: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut required_ops: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for case in &parsed.cases {
        match &case.setup {
            spec::Setup::None => {}
            spec::Setup::Single(name) => {
                required_setups.insert(name.clone());
            }
            spec::Setup::Multi(entries) => {
                for (_, fn_name) in entries {
                    required_setups.insert(fn_name.clone());
                }
            }
        }
        if !case.steps.is_empty() {
            for s in &case.steps {
                required_ops.insert(s.clone());
            }
        } else if let Some(op) = case.operation.as_deref() {
            required_ops.insert(op.to_string());
        }
    }

    // Try each candidate source: parse, score, pick the best.
    // Bonus score for a basename match so that fixtures with overlapping
    // op/setup names disambiguate by filename.
    let fixture_basename = spec_basename(&path);
    let mut best: Option<(usize, PathBuf, String, discover::Module)> = None;
    let mut had_parse_failure = false;
    for (path, src) in &candidate_sources {
        match discover::parse_module(src) {
            Ok(module) => {
                let mut score = 0usize;
                for op in &required_ops {
                    if module.method_ops.contains_key(op) || module.free_ops.contains_key(op) {
                        score += 2;
                    }
                }
                for s in &required_setups {
                    if module.setups.contains_key(s) {
                        score += 1;
                    }
                }
                if path.file_stem().and_then(|s| s.to_str()) == Some(&fixture_basename) {
                    score += 1000;
                }
                // Tie-break: file whose basename is a prefix of the spec
                // basename (e.g. `multi_field_capture` ↔ `multi_field_capture_reordered`).
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if fixture_basename.starts_with(stem) && stem.len() > 4 {
                        score += stem.len();
                    }
                }
                if best.as_ref().map(|b| score > b.0).unwrap_or(true) {
                    best = Some((score, path.clone(), src.clone(), module));
                }
            }
            Err(_) => {
                had_parse_failure = true;
            }
        }
    }

    let (_, _src_path, _src, module) = match best {
        Some(b) if b.0 > 0 => b,
        _ => {
            // No matching candidate. If the basename file exists and parses,
            // fall back to it (covers fixtures whose ops aren't found yet —
            // we'll still yield a "missing setup/operation" error below).
            let basename_path = src_dir.join(format!("{fixture_basename}.rs"));
            match std::fs::read_to_string(&basename_path) {
                Ok(s) => match discover::parse_module(&s) {
                    Ok(m) => (0, basename_path, s, m),
                    Err(_) => {
                        return RunOutcome::Error {
                            reason: "source failed to compile".into(),
                        };
                    }
                },
                Err(_) => {
                    if had_parse_failure {
                        return RunOutcome::Error {
                            reason: "source failed to compile".into(),
                        };
                    }
                    return RunOutcome::Error {
                        reason: format!("source file not found: {}", basename_path.display()),
                    };
                }
            }
        }
    };

    // Validate setup / operation references for every case.
    for case in &parsed.cases {
        match &case.setup {
            spec::Setup::None => {}
            spec::Setup::Single(name) => {
                if !module.setups.contains_key(name) {
                    return RunOutcome::Error {
                        reason: format!("setup '{name}' not found in source annotations"),
                    };
                }
            }
            spec::Setup::Multi(entries) => {
                for (_, fn_name) in entries {
                    if !module.setups.contains_key(fn_name) {
                        return RunOutcome::Error {
                            reason: format!(
                                "setup '{fn_name}' not found in source annotations"
                            ),
                        };
                    }
                }
            }
        }
        let ops_to_check: Vec<&str> = if !case.steps.is_empty() {
            case.steps.iter().map(String::as_str).collect()
        } else if let Some(op) = case.operation.as_deref() {
            vec![op]
        } else {
            vec![]
        };
        for op in ops_to_check {
            if !module.method_ops.contains_key(op) && !module.free_ops.contains_key(op) {
                return RunOutcome::Error {
                    reason: format!("operation '{op}' not found in source annotations"),
                };
            }
        }
    }

    // Run each case and collect results.
    let mut results = Vec::with_capacity(parsed.cases.len());
    for case in &parsed.cases {
        let traces = interpret::run_case(&module, case).unwrap_or_default();
        let pass = match_traces::matches(&case.expected, &traces);
        results.push(CaseResult {
            name: case.name.clone(),
            status: if pass { CaseStatus::Pass } else { CaseStatus::Fail },
            expected: case.expected.clone(),
            traces,
        });
    }

    RunOutcome::Complete { results }
}

fn spec_basename(p: &Path) -> String {
    let f = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if let Some(stripped) = f.strip_suffix(".spec.yaml") {
        return stripped.to_string();
    }
    f.trim_end_matches(".yaml").to_string()
}
