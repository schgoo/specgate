//! `SpecGate` harness — compiles annotated code, runs its operations, collects
//! runtime traces, and matches them against a spec's expected assertions.
//!
//! `run_spec(path)` loads a spec, resolves its binding, and for each case
//! generates a temporary Cargo project (the "runner") that LINKS the target
//! crate as a dependency and calls its public operations, shells out to
//! `cargo run` to compile + execute, then reads the emitted traces back and
//! subsequence-matches them against each case's `expected:` list.
//!
//! Key design points:
//! - A spec is a contract over a component's PUBLIC API, so the harness reaches
//!   each operation only through the target crate's public path
//!   (`use <crate>[::<module>] as fut;`) — it never inlines or interprets the
//!   source. A non-public annotated operation is rejected up front with a
//!   "not publicly reachable" diagnostic.
//! - It scans source only for attribute names and signatures (to validate the
//!   spec references real symbols and to know how to call them); everything
//!   else is delegated to the real Rust toolchain.
//! - Matching is a subsequence match with a rich operator set; async operations
//!   are driven on a per-target runtime (`smol` or `tokio`).

mod binding;
mod codegen;
mod coverage;
mod match_traces;
pub(crate) mod scan;
mod spec;
mod types;

// Public API — what users need for run_spec() results
pub use types::{CaseLevel, CaseResult, CaseStatus, CoverageOutcome, CoverageReport, FileCoverage, RunOutcome, Source, TargetFailure};

// Internal types — exposed for integration tests within this crate,
// but hidden from public docs. Not part of the stable API.
#[doc(hidden)]
pub use types::{AnyArg, AssertValue, Assertion, Matcher, TraceEvent, Value};

/// Resolve a binding path the same way `run_spec` does (spec-relative first,
/// then walking up parent directories). Re-exported so other tools (e.g. the
/// CLI's `validate`) resolve bindings identically to the harness, avoiding
/// drift between static validation and actual runs.
pub use spec::binding_path_resolved;

/// Source-annotation scanning and the shared runnability pre-flight. Re-exported
/// so the CLI's `validate` checks operation-annotation and setup-wiring with the
/// exact logic the harness uses before a run (single source of truth). Reaches
/// the CLI directly via its `specgate-harness` dependency, without widening the
/// `specgate` umbrella's public API.
pub use scan::{AnnotatedSource, RunnabilityIssue, RunnabilityProblem, RunnableCase, scan};

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter for unique C# scratch directory names, avoiding
/// conflicts when multiple test threads run the same spec concurrently.
static CSHARP_SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

/// Loads and validates the spec at `spec_path`, generates a temporary Cargo
/// project, compiles and runs it, then matches traces against each case's
/// `expected:` assertions.
///
/// # Panics
///
/// Panics only if an internal invariant is violated: all target names in case
/// groups are validated before any IO work begins, so the `.unwrap()` on
/// `binding.target(...)` inside the group loop cannot be reached with an
/// unknown target.
#[must_use]
pub fn run_spec(spec_path: &str) -> RunOutcome {
    let exec = execute_spec(spec_path, false);
    match exec.outcome {
        Ok(results) => RunOutcome::Complete { results },
        Err(reason) => RunOutcome::Error { reason },
    }
}

/// Like [`run_spec`], but also measures code coverage of the implementation
/// exercised by the spec's cases. Builds and runs each compiled target group's
/// runner under `-C instrument-coverage`, then merges and reports the profiles.
///
/// # Panics
///
/// Panics only on the same internal invariant as [`run_spec`] (all target names
/// are validated before the group loop).
#[must_use]
pub fn run_spec_with_coverage(spec_path: &str) -> CoverageOutcome {
    let exec = execute_spec(spec_path, true);
    let results = match exec.outcome {
        Ok(results) => results,
        Err(reason) => return CoverageOutcome::Error { reason },
    };
    match coverage::compute(&exec.group_cov, &exec.scratch_root) {
        Ok(coverage) => CoverageOutcome::Measured { results, coverage },
        Err(reason) => CoverageOutcome::Unavailable { results, reason },
    }
}

/// Result of the shared spec-execution pipeline used by both [`run_spec`] and
/// [`run_spec_with_coverage`].
struct ExecResult {
    outcome: Result<Vec<CaseResult>, String>,
    group_cov: Vec<coverage::GroupCoverage>,
    scratch_root: PathBuf,
}

fn execute_spec(spec_path: &str, coverage: bool) -> ExecResult {
    let err = |reason: String| ExecResult {
        outcome: Err(reason),
        group_cov: Vec::new(),
        scratch_root: PathBuf::new(),
    };

    let path = PathBuf::from(spec_path);
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return err(format!("spec file not found: {spec_path}"));
    };

    // First: pure YAML validity.
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return err("spec file is not valid YAML".into()),
    };

    // Then: spec shape parsing.
    let Ok(parsed) = spec::parse_spec(&yaml_value) else {
        return err("spec file is not valid YAML".into());
    };

    if parsed.cases.is_empty() {
        return err("spec has no test cases".into());
    }

    if parsed.binding_paths.is_empty() {
        return err("spec has no binding".into());
    }

    // Load all bindings. First is canonical.
    let mut bindings: Vec<(String, binding::Binding)> = Vec::new();
    for bp in &parsed.binding_paths {
        let binding_full = binding_path_resolved(&path, bp);
        let Some(b) = binding::load_binding(&binding_full) else {
            return err(format!("binding '{bp}' not found"));
        };
        let name = Path::new(bp).file_stem().and_then(|s| s.to_str()).unwrap_or(bp).to_string();
        bindings.push((name, b));
    }

    // Shape check: spec-level event key validation.
    if let Some(reason) = check_shape(&parsed, &yaml_value) {
        return err(reason);
    }

    let fixture_basename = spec_basename(&path);
    let workspace_root = workspace_root();

    // Group cases by effective target (case.target ?? spec.target ?? None),
    // preserving first-appearance order.
    let mut groups: Vec<(Option<String>, Vec<usize>)> = Vec::new();
    {
        let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (i, case) in parsed.cases.iter().enumerate() {
            let eff = case.target.clone().or_else(|| parsed.target.clone());
            // Use a sentinel key that can't clash with real target names.
            let key = eff.as_deref().map_or_else(|| "\x00default\x00".to_string(), String::from);
            if let Some(&gidx) = seen.get(&key) {
                groups[gidx].1.push(i);
            } else {
                seen.insert(key, groups.len());
                groups.push((eff, vec![i]));
            }
        }
    }

    // Validate every target exists in every binding before doing any IO-heavy work.
    for (_binding_name, binding) in &bindings {
        for (eff_target, _) in &groups {
            let target_name = eff_target.as_deref();
            if binding.target(target_name).is_none() {
                return err(format!("target '{}' not found in binding", target_name.unwrap_or("<default>")));
            }
        }
    }

    // Process each binding and accumulate per-case results indexed by case index.
    // all_binding_results[binding_idx][case_idx] = Option<CaseResult>
    let mut all_binding_results: Vec<Vec<Option<CaseResult>>> = Vec::new();
    let mut all_target_labels: Vec<Vec<Option<String>>> = Vec::new();
    let mut group_cov: Vec<coverage::GroupCoverage> = Vec::new();

    for (binding_idx, (binding_name, binding)) in bindings.iter().enumerate() {
        let mut results_by_index: Vec<Option<CaseResult>> = vec![None; parsed.cases.len()];
        let mut target_labels_by_index: Vec<Option<String>> = vec![None; parsed.cases.len()];

        for (eff_target, case_indices) in &groups {
            let target = binding.target(eff_target.as_deref()).unwrap();
            let group_cases: Vec<&spec::Case> = case_indices.iter().map(|&i| &parsed.cases[i]).collect();

            // Give each (binding, target) pair a distinct scratch directory.
            // The canonical (index 0) binding uses the original naming for backward compat.
            // Non-canonical bindings (e.g. C#) get a per-invocation unique ID to prevent
            // concurrent test runs from colliding on the same build artifacts.
            let scratch_suffix = if binding_idx == 0 {
                match eff_target.as_deref() {
                    None => fixture_basename.clone(),
                    Some(t) => format!("{fixture_basename}_{t}"),
                }
            } else {
                let uid = CSHARP_SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
                let pid = std::process::id();
                match eff_target.as_deref() {
                    None => format!("{fixture_basename}_{binding_name}_{pid}_{uid}"),
                    Some(t) => format!("{fixture_basename}_{binding_name}_{t}_{pid}_{uid}"),
                }
            };
            let scratch_dir = scratch_for(&scratch_suffix);

            let group_result = if binding.language == "csharp" {
                run_csharp_group(target, &group_cases, &parsed, &fixture_basename, &scratch_dir).map(|r| (r, None))
            } else {
                run_group(
                    target,
                    &group_cases,
                    &parsed,
                    &fixture_basename,
                    &workspace_root,
                    &scratch_dir,
                    coverage,
                )
            };

            match group_result {
                Ok((group_results, cov)) => {
                    for (&case_idx, result) in case_indices.iter().zip(group_results) {
                        results_by_index[case_idx] = Some(result);
                        target_labels_by_index[case_idx] = Some(target_label(binding_name, eff_target.as_deref()));
                    }
                    if let Some(cov) = cov {
                        group_cov.push(cov);
                    }
                }
                Err(reason) => {
                    return ExecResult {
                        outcome: Err(reason),
                        group_cov,
                        scratch_root: scratch_for(&fixture_basename),
                    };
                }
            }
        }

        all_binding_results.push(results_by_index);
        all_target_labels.push(target_labels_by_index);
    }

    // Merge results: the canonical binding (index 0) supplies the primary traces.
    // Non-canonical bindings contribute TargetFailure entries when their traces
    // diverge from the canonical traces.
    let n = parsed.cases.len();
    let (canonical_results, other_results) = all_binding_results.split_first_mut().expect("at least one binding");
    let (_, other_target_labels) = all_target_labels.split_first_mut().expect("at least one binding");

    let mut results = Vec::with_capacity(n);
    for i in 0..n {
        let canonical = canonical_results[i].take().expect("all case indices covered by groups");
        let others: Vec<(String, CaseResult)> = other_results
            .iter_mut()
            .zip(other_target_labels.iter())
            .filter_map(|(binding_results, labels)| {
                let label = labels[i].clone().unwrap_or_else(|| "<unknown>".to_string());
                binding_results[i].take().map(|r| (label, r))
            })
            .collect();
        results.push(merge_target_results(canonical, others));
    }

    ExecResult {
        outcome: Ok(results),
        group_cov,
        scratch_root: scratch_for(&fixture_basename),
    }
}

/// Run one target group: resolve source, validate annotations, generate a
/// temporary runner, compile + execute it, and return per-case results. When
/// `coverage` is set, the runner is built/run under instrumentation and the
/// captured artifacts are returned alongside the results.
fn run_group(
    target: &binding::Target,
    group_cases: &[&spec::Case],
    spec: &spec::Spec,
    fixture_basename: &str,
    workspace_root: &Path,
    scratch_dir: &Path,
    coverage: bool,
) -> Result<(Vec<CaseResult>, Option<coverage::GroupCoverage>), String> {
    // Command target: run the binding's shell command once and map its exit
    // status to a synthetic `$outcome` event (exit 0 -> "Complete", else
    // "Error"), then match each case. No source file is resolved or compiled,
    // so command targets contribute no coverage data.
    if let Some(command) = target.command.as_deref() {
        return run_command_group(command, &target.package_root, group_cases).map(|r| (r, None));
    }

    let fixture_srcs = resolve_fixture_sources(&target.package_root, fixture_basename, group_cases);
    if fixture_srcs.is_empty() {
        if let Some(results) = short_circuit_non_must(group_cases, None) {
            return Ok((results, None));
        }
        return Err(format!(
            "source file not found: {}",
            target.package_root.join("src").join(format!("{fixture_basename}.rs")).display()
        ));
    }

    // Merge the text of every contributing source so operations split across
    // separate files are scanned together.
    let mut src_text = String::new();
    for fs in &fixture_srcs {
        src_text.push_str(&load_fixture_text(fs)?);
        src_text.push('\n');
    }

    let annotated = scan(&src_text);

    // Pre-flight runnability: every operation a MUST case runs must be
    // annotated, and each case's setups must wire. Shared with the CLI's
    // `validate` (scan::check_runnable) so static validation and runs agree.
    // Returns a precise diagnostic before any code generation or compilation.
    let runnable_cases: Vec<RunnableCase> = group_cases
        .iter()
        .map(|c| {
            let ops = if c.steps.is_empty() {
                c.operation.clone().into_iter().collect()
            } else {
                c.steps.clone()
            };
            RunnableCase {
                name: c.name.clone(),
                ops,
                is_must: c.level == CaseLevel::Must,
            }
        })
        .collect();
    if let Some(issue) = annotated.check_runnable(&runnable_cases).into_iter().next() {
        return Err(match issue.problem {
            RunnabilityProblem::OperationNotAnnotated { operation } => {
                format!("operation '{operation}' not found in source annotations")
            }
            RunnabilityProblem::SetupWiring { detail } => format!("case '{}': {detail}", issue.case),
        });
    }

    // Decide which cases run via cargo vs short-circuit (skip/warn).
    let mut case_disposition: Vec<CaseDisposition> = Vec::with_capacity(group_cases.len());
    let mut runnable = false;
    for case in group_cases {
        let disp = if case_pieces_available(case, &annotated) {
            runnable = true;
            CaseDisposition::Run
        } else {
            match case.level {
                CaseLevel::Must => {
                    runnable = true;
                    CaseDisposition::Run
                }
                CaseLevel::Should => CaseDisposition::Warn,
                CaseLevel::May => CaseDisposition::Skip,
            }
        };
        case_disposition.push(disp);
    }

    if !runnable {
        return Ok((build_short_circuit_results(group_cases, &case_disposition), None));
    }

    let cases_to_run: Vec<&spec::Case> = group_cases
        .iter()
        .zip(case_disposition.iter())
        .filter_map(|(&c, d)| matches!(d, CaseDisposition::Run).then_some(c))
        .collect();
    let needs_async = cases_to_run.iter().any(|c| case_uses_async(c, spec));

    // Link-only pre-flight: every operation the cases run must be publicly
    // reachable through the target crate's public path
    // (`use <crate>[::<module>] as fut;`). An operation whose module IS a public
    // path (`pub mod`-declared or the crate root) but whose implementing fn is
    // not `pub` is rejected here with a clean "not publicly reachable"
    // diagnostic, BEFORE any scaffolding/compilation — rather than surfacing as
    // a raw cargo compile error.
    //
    // A module that is NOT a public path (e.g. an undeclared or commented-out
    // `pub mod`) is intentionally NOT rejected here: linking it produces the
    // target's own compiler diagnostics (a "source failed to compile" error),
    // preserving the compile-failure contract.
    let mut required_ops: Vec<&str> = Vec::new();
    for c in &cases_to_run {
        let ops: Vec<&str> = if c.steps.is_empty() {
            c.operation.as_deref().into_iter().collect()
        } else {
            c.steps.iter().map(String::as_str).collect()
        };
        for op in ops {
            if !required_ops.contains(&op) {
                required_ops.push(op);
            }
        }
    }
    for src in &fixture_srcs {
        let module = src.file_stem().and_then(|s| s.to_str()).unwrap_or("fixture");
        if !codegen::module_publicly_linkable(&target.package_root, module) {
            continue;
        }
        let src_annotated = scan(&load_fixture_text(src)?);
        for op in &required_ops {
            if let Some(decl) = src_annotated.operations.get(*op)
                && !decl.is_pub
            {
                return Err(format!(
                    "spec operation '{op}' is not publicly reachable in crate '{}': \
                     its implementing function is not declared `pub`",
                    codegen::crate_label(&target.package_root)
                ));
            }
        }
    }
    let fixture_crates = codegen::resolve_fixture_crates(&target.package_root, &fixture_srcs)?;

    let proj = codegen::generate(
        scratch_dir,
        &codegen::GenerateConfig {
            spec,
            cases_to_run: &cases_to_run,
            annotated: &annotated,
            workspace_root,
            needs_async,
            runtime: target.runtime,
            fixture_crates: &fixture_crates,
            is_local: is_local_workspace(),
        },
    )
    .map_err(|e| format!("failed to scaffold runner: {e}"))?;

    // Shell out: cargo run -- <trace_out>
    let mut cmd = Command::new(cargo_bin());
    cmd.arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(proj.crate_dir.join("Cargo.toml"))
        .arg("--")
        .arg(&proj.trace_file);
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
    cmd.env("CARGO_TARGET_DIR", proj.crate_dir.join("target").as_os_str());
    // Anchor the runner's working directory to the repo root so that
    // path-input operations resolve repo-root-relative paths deterministically,
    // independent of where the harness itself was invoked from.
    cmd.current_dir(repo_root());

    // Under coverage, build + run the runner with instrumentation enabled so the
    // profiles capture the implementation crate it links.
    let cov_profraw_dir = if coverage {
        let (env, profraw_dir) = coverage::instrumentation_env(scratch_dir);
        for (k, v) in env {
            cmd.env(k, v);
        }
        Some(profraw_dir)
    } else {
        None
    };

    let output = cmd.output().map_err(|e| format!("failed to invoke cargo: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("error[E") || stderr.contains("error:") || stderr.contains("could not compile") {
            // Surface the actual compiler diagnostics (first 20 lines) so the
            // failure is debuggable without hunting for the scratch directory.
            let detail = stderr.lines().take(20).collect::<Vec<_>>().join("\n");
            return Err(format!("source failed to compile:\n{detail}"));
        }
        return Err(format!("runner failed: {stderr}"));
    }

    let trace_text = std::fs::read_to_string(&proj.trace_file).map_err(|e| format!("runner produced no trace output: {e}"))?;
    let trace_map: BTreeMap<String, Vec<TraceEvent>> =
        serde_yaml::from_str(&trace_text).map_err(|e| format!("failed to parse traces: {e}"))?;

    let mut results = Vec::with_capacity(group_cases.len());
    for (case, disp) in group_cases.iter().zip(case_disposition.iter()) {
        match disp {
            CaseDisposition::Skip => results.push(CaseResult {
                name: case.name.clone(),
                status: CaseStatus::Skip,
                level: case.level,
                source: case.source.clone(),
                expected: Vec::new(),
                traces: Vec::new(),
                target_failures: Vec::new(),
            }),
            CaseDisposition::Warn => results.push(CaseResult {
                name: case.name.clone(),
                status: CaseStatus::Warn,
                level: case.level,
                source: case.source.clone(),
                expected: Vec::new(),
                traces: Vec::new(),
                target_failures: Vec::new(),
            }),
            CaseDisposition::Run => {
                let traces = trace_map.get(&case.name).cloned().unwrap_or_default();
                let pass = match_traces::matches(&case.expected, &traces);
                results.push(CaseResult {
                    name: case.name.clone(),
                    status: if pass { CaseStatus::Pass } else { CaseStatus::Fail },
                    level: case.level,
                    source: case.source.clone(),
                    expected: case.expected.clone(),
                    traces,
                    target_failures: Vec::new(),
                });
            }
        }
    }

    let group_cov = cov_profraw_dir.map(|profraw_dir| {
        let exe = if cfg!(windows) { "sg-runner.exe" } else { "sg-runner" };
        coverage::GroupCoverage {
            binary: proj.crate_dir.join("target").join("debug").join(exe),
            profraw_dir,
            // Scope coverage to the whole crate under test (every `.rs` under
            // the target's package_root), not just the files bound to this
            // spec's operations. `compute` unions and dedups across groups, so a
            // multi-target spec reports the union of its crates under test.
            sources: coverage::collect_crate_sources(&target.package_root),
        }
    });
    Ok((results, group_cov))
}

// ---------------------------------------------------------------------------
// Command targets
// ---------------------------------------------------------------------------

/// Run a command-target group: execute the binding's shell command once in the
/// target's `package_root`, map its exit status to a synthetic `$outcome` event
/// ("Complete" on exit 0, "Error" otherwise), then match each case against it.
fn run_command_group(command: &str, package_root: &Path, group_cases: &[&spec::Case]) -> Result<Vec<CaseResult>, String> {
    let success = run_shell_command(command, package_root).map_err(|e| format!("failed to run command target '{command}': {e}"))?;
    let outcome = if success { "Complete" } else { "Error" };
    let traces = vec![TraceEvent::Event {
        name: "$outcome".to_string(),
        value: Value::String(outcome.to_string()),
    }];
    Ok(group_cases
        .iter()
        .map(|case| {
            let pass = match_traces::matches(&case.expected, &traces);
            CaseResult {
                name: case.name.clone(),
                status: if pass { CaseStatus::Pass } else { CaseStatus::Fail },
                level: case.level,
                source: case.source.clone(),
                expected: case.expected.clone(),
                traces: traces.clone(),
                target_failures: Vec::new(),
            }
        })
        .collect())
}

/// Execute `command` through the platform shell in `cwd`, returning whether it
/// exited successfully. Output is captured (not streamed) to keep harness output
/// clean; only the exit status is used.
fn run_shell_command(command: &str, cwd: &Path) -> std::io::Result<bool> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C");
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c");
        c
    };
    cmd.arg(command).current_dir(cwd);
    Ok(cmd.output()?.status.success())
}

fn target_label(binding_name: &str, target_name: Option<&str>) -> String {
    match target_name {
        Some(target) => format!("{binding_name}:{target}"),
        None => binding_name.to_string(),
    }
}

// ---------------------------------------------------------------------------
// C# targets
// ---------------------------------------------------------------------------

/// Metadata about a C# operation found by scanning source files.
struct CsOp {
    op_name: String,
    class_name: String,
    method_name: String,
    /// `(parameter_name, cs_type)`
    params: Vec<(String, String)>,
    return_type: String,
}

/// Scan all `.cs` files under `package_root` recursively for `[SpecOperation("name")]`
/// annotations and extract the annotated method's class, name, params, and return type.
fn scan_csharp(package_root: &Path) -> Vec<CsOp> {
    let mut ops = Vec::new();
    scan_csharp_dir(package_root, &mut ops);
    ops
}

fn scan_csharp_dir(dir: &Path, ops: &mut Vec<CsOp>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_csharp_dir(&p, ops);
        } else if p.extension().and_then(|e| e.to_str()) == Some("cs")
            && let Ok(text) = std::fs::read_to_string(&p)
        {
            scan_csharp_file(&text, ops);
        }
    }
}

fn scan_csharp_file(text: &str, ops: &mut Vec<CsOp>) {
    let mut current_class: Option<String> = None;
    let mut pending_op: Option<String> = None;

    for line in text.lines() {
        let trimmed = line.trim();

        // Detect class declaration: look for keyword "class" followed by the name.
        if let Some(class_name) = extract_cs_class(trimmed) {
            current_class = Some(class_name);
        }

        // Detect [SpecOperation("name")]
        if let Some(op_name) = extract_spec_operation_attr(trimmed) {
            pending_op = Some(op_name);
            continue;
        }

        // If we have a pending [SpecOperation], try to parse the next method signature.
        if let Some(op_name) = pending_op.take()
            && let Some((method_name, params, return_type)) = extract_cs_method_sig(trimmed)
        {
            ops.push(CsOp {
                op_name,
                class_name: current_class.clone().unwrap_or_default(),
                method_name,
                params,
                return_type,
            });
            // If the line wasn't a method signature, the pending op is discarded.
        }
    }
}

fn extract_cs_class(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let idx = words.iter().position(|&w| w == "class")?;
    let raw = words.get(idx + 1)?;
    // Strip trailing '{', ':', '<T>' etc.
    let name: String = raw.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_spec_operation_attr(line: &str) -> Option<String> {
    let s = line.trim();
    let rest = s.strip_prefix("[SpecOperation(\"")?;
    let name = rest.split('"').next()?;
    Some(name.to_string())
}

#[allow(clippy::type_complexity)]
fn extract_cs_method_sig(line: &str) -> Option<(String, Vec<(String, String)>, String)> {
    let paren_open = line.find('(')?;
    let paren_close = find_matching_paren(line, paren_open)?;

    let before = line[..paren_open].trim();
    let parts: Vec<&str> = before.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let method_name = parts.last()?.to_string();
    let return_type = parts[parts.len() - 2].to_string();

    // Must look like an identifier (not a keyword like "if", "{" etc.)
    if !method_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }

    let params_str = line[paren_open + 1..paren_close].trim();
    let params = if params_str.is_empty() {
        Vec::new()
    } else {
        split_cs_params(params_str).into_iter().filter_map(parse_cs_param).collect()
    };

    Some((method_name, params, return_type))
}

fn split_cs_params(params: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut generic_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in params.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '<' => generic_depth += 1,
            '>' => generic_depth = generic_depth.saturating_sub(1),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if generic_depth == 0 && paren_depth == 0 && bracket_depth == 0 => {
                out.push(params[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    let tail = params[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn find_matching_paren(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open) {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(idx);
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_cs_param(param: &str) -> Option<(String, String)> {
    let (spec_name, without_attrs) = peel_spec_input_attrs(param.trim());
    let parts: Vec<&str> = without_attrs.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let code_name = parts.last()?.trim_start_matches('@');
    let name = spec_name.unwrap_or_else(|| code_name.to_string());
    Some((name, parts[..parts.len() - 1].join(" ")))
}

fn peel_spec_input_attrs(mut param: &str) -> (Option<String>, &str) {
    let mut spec_name = None;
    loop {
        let p = param.trim_start();
        if !p.starts_with('[') {
            return (spec_name, p);
        }
        let Some(end) = p.find(']') else {
            return (spec_name, p);
        };
        let attr = &p[..=end];
        if spec_name.is_none()
            && let Some(name) = extract_spec_input_attr(attr)
        {
            spec_name = Some(name);
        }
        param = &p[end + 1..];
    }
}

fn extract_spec_input_attr(attr: &str) -> Option<String> {
    let open = attr.find("SpecInput(\"")?;
    let rest = &attr[open + "SpecInput(\"".len()..];
    let name = rest.split('"').next()?;
    Some(name.to_string())
}

/// Convert a YAML value to a C# literal string for the given C# parameter type.
fn yaml_to_csharp_literal(val: Option<&serde_yaml::Value>, param_type: &str) -> String {
    let Some(v) = val else { return "default!".to_string() };
    if v.as_mapping().is_some() || v.as_sequence().is_some() {
        let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
        return format!("FromJson<{}>({})", param_type.trim(), csharp_string_literal(&json));
    }
    if let Some(i) = v.as_i64() {
        return i.to_string();
    }
    if let Some(f) = v.as_f64() {
        return f.to_string();
    }
    if let Some(b) = v.as_bool() {
        return if b { "true".to_string() } else { "false".to_string() };
    }
    if let Some(s) = v.as_str() {
        return csharp_string_literal(s);
    }
    "default!".to_string()
}

fn csharp_string_literal(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

/// True when `cs_type` is a C# integer primitive that maps to the `EmitEvent(string, int)`
/// overload. Integer types other than `int` are cast to `int` because the overload
/// serializes them as unquoted JSON numbers, matching Rust's `Value::Integer` wire format.
fn is_cs_integer_type(cs_type: &str) -> bool {
    matches!(
        cs_type.trim(),
        "int" | "long" | "uint" | "ulong" | "short" | "ushort" | "byte" | "sbyte" | "Int32" | "Int64"
    )
}

/// Generate a typed `SpecGateRuntime.EmitEvent(…)` call for the given C# variable
/// and type. Integers use the `(string, int)` overload (unquoted JSON number);
/// booleans use the `(string, bool)` overload (unquoted `true`/`false`);
/// strings use the `(string, string)` overload (quoted JSON string).
/// All other types fall back to `.ToString()` via the string overload.
fn cs_typed_emit(event_name: &str, var: &str, cs_type: &str) -> String {
    let name_lit = format!("\"{event_name}\"");
    let t = cs_type.trim();
    if t == "int" {
        format!("SpecGateRuntime.EmitEvent({name_lit}, {var});")
    } else if is_cs_integer_type(t) {
        format!("SpecGateRuntime.EmitEvent({name_lit}, (int){var});")
    } else {
        format!("SpecGateRuntime.EmitEvent({name_lit}, {var});")
    }
}

/// Convert a Path to a forward-slash string (for use in C# project files).
fn path_to_forward_slash(p: &Path) -> String {
    let path = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| p.to_path_buf(), |cwd| cwd.join(p))
    };
    let s = path.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

/// Walk up from `package_root` to find the nearest ancestor that contains a `csharp` child
/// directory holding the `SpecGate` production library projects.
/// Returns the path to `<ancestor>/csharp` if found, or `None` if no such ancestor exists.
fn find_csharp_libs_dir(package_root: &Path) -> Option<PathBuf> {
    let mut dir = package_root.to_path_buf();
    loop {
        let candidate = dir.join("csharp");
        if candidate.join("SpecGate.Annotations").join("SpecGate.Annotations.csproj").exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Generate the `Program.cs` content for the C# runner.
fn generate_csharp_program(
    cases: &[&spec::Case],
    cs_ops: &[CsOp],
    op_input_defaults: &BTreeMap<String, BTreeMap<String, serde_yaml::Value>>,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("using SpecGate.Annotations;\n");
    out.push_str("using SpecGate.Runtime;\n");
    out.push_str("using SpecGateFixtures;\n");
    out.push_str("using System.Collections.Generic;\n");
    out.push_str("using System.IO;\n");
    out.push_str("using System.Text;\n\n");
    out.push_str("using System.Text.Json;\n\n");
    out.push_str("var all = new SortedDictionary<string, string>();\n");

    for case in cases {
        let op_name = case.operation.as_deref().unwrap_or("");
        if let Some(cs_op) = cs_ops.iter().find(|o| o.op_name == op_name) {
            let op_defaults = op_input_defaults.get(op_name);
            writeln!(out, "// case: {}", case.name).expect("fmt");
            out.push_str("{\n");
            out.push_str("    SpecGateRuntime.Reset();\n");
            writeln!(out, "    SpecGateRuntime.EmitRun(\"{op_name}\");").expect("fmt");

            for (param_name, param_type) in &cs_op.params {
                let case_val = case.inputs.get(param_name);
                let val = case_val.or_else(|| op_defaults.and_then(|d| d.get(param_name)));
                let lit = yaml_to_csharp_literal(val, param_type);
                writeln!(out, "    {param_type} __{param_name} = {lit};").expect("fmt");
            }
            for (param_name, param_type) in &cs_op.params {
                let emit = cs_typed_emit(&format!("{op_name}.{param_name}"), &format!("__{param_name}"), param_type);
                writeln!(out, "    {emit}").expect("fmt");
            }

            let args = cs_op.params.iter().map(|(n, _)| format!("__{n}")).collect::<Vec<_>>().join(", ");
            out.push_str("    try {\n");
            writeln!(
                out,
                "        {} __result = {}.{}({args});",
                cs_op.return_type, cs_op.class_name, cs_op.method_name
            )
            .expect("fmt");
            if cs_op.return_type.trim() != "void" {
                writeln!(out, "        SpecGateRuntime.EmitResult(__result);").expect("fmt");
            }
            out.push_str("    } catch (System.Exception __ex) {\n");
            out.push_str("        SpecGateRuntime.EmitEvent(\"$fault\", __ex.Message);\n");
            out.push_str("    }\n");
            writeln!(out, "    all[\"{}\"] = SpecGateRuntime.GetTracesJson();", case.name).expect("fmt");
            out.push_str("}\n");
        }
    }

    out.push_str("var sb = new StringBuilder(\"{\");\n");
    out.push_str("bool first = true;\n");
    out.push_str("foreach (var kv in all) {\n");
    out.push_str("    if (!first) sb.Append(',');\n");
    out.push_str("    first = false;\n");
    out.push_str("    sb.Append('\"');\n");
    out.push_str("    AppendJsonString(sb, kv.Key);\n");
    out.push_str("    sb.Append(\"\\\":\");\n");
    out.push_str("    sb.Append(kv.Value);\n");
    out.push_str("}\n");
    out.push_str("sb.Append('}');\n");
    out.push_str("File.WriteAllText(args[0], sb.ToString());\n\n");
    out.push_str("static T FromJson<T>(string json) => JsonSerializer.Deserialize<T>(json, new JsonSerializerOptions { IncludeFields = true })!;\n\n");
    out.push_str("static void AppendJsonString(StringBuilder sb, string s) {\n");
    out.push_str("    foreach (char c in s) {\n");
    out.push_str("        switch (c) {\n");
    out.push_str("            case '\"': sb.Append(\"\\\\\\\"\"); break;\n");
    out.push_str("            case '\\\\': sb.Append(\"\\\\\\\\\"); break;\n");
    out.push_str("            case '\\n': sb.Append(\"\\\\n\"); break;\n");
    out.push_str("            case '\\r': sb.Append(\"\\\\r\"); break;\n");
    out.push_str("            case '\\t': sb.Append(\"\\\\t\"); break;\n");
    out.push_str("            default: sb.Append(c); break;\n");
    out.push_str("        }\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

/// Extract the value of a single XML element `<tag>content</tag>` from a
/// `.csproj`-style XML string. Returns `None` when the tag is absent.
/// Whitespace around the content is trimmed.
pub(crate) fn extract_csproj_xml_tag(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = text.find(&open)?;
    let inner_start = start + open.len();
    let end = text[inner_start..].find(&close)?;
    Some(text[inner_start..inner_start + end].trim().to_string())
}

/// Find the first `.csproj` in `package_root` and return the effective
/// target framework moniker declared in it.
///
/// Checks `<TargetFramework>` first; then `<TargetFrameworks>` (returns
/// the first semicolon-delimited item). Returns `None` when no `.csproj`
/// exists or neither element is present.
pub(crate) fn read_csproj_framework(package_root: &Path) -> Option<String> {
    let entries = std::fs::read_dir(package_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("csproj") {
            let text = std::fs::read_to_string(&path).ok()?;
            if let Some(fw) = extract_csproj_xml_tag(&text, "TargetFramework") {
                return Some(fw);
            }
            if let Some(fws) = extract_csproj_xml_tag(&text, "TargetFrameworks") {
                return fws.split(';').next().map(|s| s.trim().to_string());
            }
        }
    }
    None
}

/// Resolve the `<TargetFramework>` to use in the generated `Runner.csproj`.
///
/// Resolution order:
/// 1. `target.framework` if set in the binding.
/// 2. The framework declared in the target project's `.csproj`
///    (`<TargetFramework>` or first item of `<TargetFrameworks>`).
/// 3. Default: `net10.0`.
///
/// If the resolved value starts with `netstandard` (a library-only TFM that
/// cannot produce an executable), the runner falls back to `net10.0`.
pub(crate) fn resolve_framework_for_runner(target: &binding::Target) -> String {
    const DEFAULT: &str = "net10.0";
    let fw = target
        .framework
        .clone()
        .or_else(|| read_csproj_framework(&target.package_root))
        .unwrap_or_else(|| DEFAULT.to_string());
    if fw.starts_with("netstandard") { DEFAULT.to_string() } else { fw }
}

/// Run one C# target group: scan annotated operations, generate and execute a
/// temporary C# runner, parse its trace output, and return per-case results.
fn run_csharp_group(
    target: &binding::Target,
    group_cases: &[&spec::Case],
    spec: &spec::Spec,
    _fixture_basename: &str,
    scratch_dir: &Path,
) -> Result<Vec<CaseResult>, String> {
    let cs_ops = scan_csharp(&target.package_root);

    // Verify all required operations have C# annotations.
    let mut required_ops: Vec<&str> = Vec::new();
    for case in group_cases {
        let ops: Vec<&str> = if case.steps.is_empty() {
            case.operation.as_deref().into_iter().collect()
        } else {
            case.steps.iter().map(String::as_str).collect()
        };
        for op in ops {
            if !required_ops.contains(&op) {
                required_ops.push(op);
            }
        }
    }
    for op in &required_ops {
        if !cs_ops.iter().any(|co| co.op_name == *op) {
            return Err(format!("C# operation '{op}' not found in source annotations"));
        }
        let matching = cs_ops.iter().filter(|co| co.op_name == *op).count();
        if matching > 1 {
            return Err(format!("C# operation '{op}' has {matching} matching source annotations"));
        }
    }

    std::fs::create_dir_all(scratch_dir).map_err(|e| format!("failed to create C# scratch dir: {e}"))?;
    let trace_file = scratch_dir.join("traces.json");

    // Resolve the target framework for the runner (binding > csproj > default,
    // with netstandard falling back to net10.0 since it can't produce an exe).
    let runner_tfm = resolve_framework_for_runner(target);

    // Write Runner.csproj that compiles the fixture source directly. This keeps
    // concurrent harness runs from contending on the fixture project's obj/bin.
    let pkg_path = path_to_forward_slash(&target.package_root);
    let csharp_libs_dir = find_csharp_libs_dir(&target.package_root);
    let runtime_sources = match &csharp_libs_dir {
        Some(libs) => {
            let annotations = path_to_forward_slash(&libs.join("SpecGate.Annotations").join("SpecGateAnnotations.cs"));
            let runtime = path_to_forward_slash(&libs.join("SpecGate.Runtime").join("SpecGateRuntime.cs"));
            format!(
                "  <ItemGroup>\n    \
                 <Compile Include=\"{annotations}\" Link=\"SpecGateAnnotations.cs\" />\n    \
                 <Compile Include=\"{runtime}\" Link=\"SpecGateRuntime.cs\" />\n  \
                 </ItemGroup>\n"
            )
        }
        None => String::new(),
    };
    let csproj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    <TargetFramework>{runner_tfm}</TargetFramework>\n    \
         <Nullable>enable</Nullable>\n  </PropertyGroup>\n  <ItemGroup>\n    \
         <Compile Include=\"{pkg_path}/**/*.cs\" Exclude=\"{pkg_path}/Tests/**/*.cs;{pkg_path}/bin/**/*.cs;{pkg_path}/obj/**/*.cs\" LinkBase=\"Fixture\" />\n  \
         </ItemGroup>\n{runtime_sources}</Project>\n"
    );
    std::fs::write(scratch_dir.join("Runner.csproj"), csproj).map_err(|e| format!("failed to write Runner.csproj: {e}"))?;

    // Write Program.cs.
    let program_cs = generate_csharp_program(group_cases, &cs_ops, &spec.op_input_defaults);
    std::fs::write(scratch_dir.join("Program.cs"), program_cs).map_err(|e| format!("failed to write Program.cs: {e}"))?;

    // Run: dotnet run --project Runner.csproj -- <trace_file>
    let mut cmd = Command::new("dotnet");
    cmd.arg("run")
        .arg("--project")
        .arg(scratch_dir.join("Runner.csproj"))
        .arg("--")
        .arg(&trace_file)
        // Anchor working dir to scratch so dotnet is independent of process cwd.
        .current_dir(scratch_dir);

    let output = cmd.output().map_err(|e| format!("failed to invoke dotnet: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let combined = format!("{stderr}\n{stdout}");
        return Err(format!(
            "C# runner failed:\n{}",
            combined.lines().take(30).collect::<Vec<_>>().join("\n")
        ));
    }

    let trace_text = std::fs::read_to_string(&trace_file).map_err(|e| format!("C# runner produced no trace output: {e}"))?;
    let trace_map: BTreeMap<String, Vec<TraceEvent>> =
        serde_yaml::from_str(&trace_text).map_err(|e| format!("failed to parse C# traces: {e}"))?;

    let mut results = Vec::with_capacity(group_cases.len());
    for case in group_cases {
        let traces = trace_map.get(&case.name).cloned().unwrap_or_default();
        let pass = match_traces::matches(&case.expected, &traces);
        results.push(CaseResult {
            name: case.name.clone(),
            status: if pass { CaseStatus::Pass } else { CaseStatus::Fail },
            level: case.level,
            source: case.source.clone(),
            expected: case.expected.clone(),
            traces,
            target_failures: Vec::new(),
        });
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Multi-binding result merge
// ---------------------------------------------------------------------------

/// Merge results from multiple bindings for a single case.
///
/// A [`TargetFailure`] is recorded whenever a non-canonical target's traces
/// diverge from the canonical traces (`other.traces != canonical.traces`).
/// If any divergence is found, the merged status is set to [`CaseStatus::Fail`]
/// regardless of the canonical binding's original status.
fn merge_target_results(mut canonical: CaseResult, others: Vec<(String, CaseResult)>) -> CaseResult {
    let mut target_failures = Vec::new();
    for (target_label, other) in others {
        if other.traces != canonical.traces {
            let mismatch = format!(
                "trace diverges from canonical: canonical has {} events, target has {}",
                canonical.traces.len(),
                other.traces.len()
            );
            target_failures.push(TargetFailure {
                target: target_label,
                traces: other.traces,
                mismatch,
            });
        }
    }
    if !target_failures.is_empty() {
        canonical.status = CaseStatus::Fail;
    }
    canonical.target_failures = target_failures;
    canonical
}

// ---------------------------------------------------------------------------
// Per-case classification helpers
// ---------------------------------------------------------------------------

enum CaseDisposition {
    Run,
    Skip,
    Warn,
}

fn case_pieces_available(case: &spec::Case, annotated: &AnnotatedSource) -> bool {
    let ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        return true;
    };
    if !ops.iter().all(|o| annotated.operations.contains_key(*o)) {
        return false;
    }
    // The operation's setups must resolve (e.g. a method receiver has a setup).
    annotated.resolve_case(&ops).is_ok()
}

/// If every case has level != MUST, return per-case warn/skip results.
fn short_circuit_non_must(cases: &[&spec::Case], _annotated: Option<&AnnotatedSource>) -> Option<Vec<CaseResult>> {
    if cases.iter().any(|c| c.level == CaseLevel::Must) {
        return None;
    }
    let mut out = Vec::with_capacity(cases.len());
    for c in cases {
        let status = match c.level {
            CaseLevel::Should => CaseStatus::Warn,
            CaseLevel::May => CaseStatus::Skip,
            CaseLevel::Must => unreachable!(),
        };
        out.push(CaseResult {
            name: c.name.clone(),
            status,
            level: c.level,
            source: c.source.clone(),
            expected: Vec::new(),
            traces: Vec::new(),
            target_failures: Vec::new(),
        });
    }
    Some(out)
}

fn build_short_circuit_results(cases: &[&spec::Case], disp: &[CaseDisposition]) -> Vec<CaseResult> {
    cases
        .iter()
        .zip(disp.iter())
        .map(|(c, d)| {
            let status = match d {
                CaseDisposition::Skip => CaseStatus::Skip,
                CaseDisposition::Warn => CaseStatus::Warn,
                CaseDisposition::Run => unreachable!("runnable case in short-circuit path"),
            };
            CaseResult {
                name: c.name.clone(),
                status,
                level: c.level,
                source: c.source.clone(),
                expected: Vec::new(),
                traces: Vec::new(),
                target_failures: Vec::new(),
            }
        })
        .collect()
}

fn case_uses_async(case: &spec::Case, spec: &spec::Spec) -> bool {
    let ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        return false;
    };
    ops.iter().any(|o| spec.async_ops.contains(*o))
}

fn spec_basename(p: &Path) -> String {
    let f = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if let Some(stripped) = f.strip_suffix(".spec.yaml") {
        return stripped.to_string();
    }
    f.trim_end_matches(".yaml").to_string()
}

fn workspace_root() -> PathBuf {
    // env! resolves at compile time — always points to specgate-harness's
    // directory, even when the harness is used as a dependency from an
    // external project.
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p
}

/// The repository root — the parent of the Rust workspace. Used as the working
/// directory for the generated runner so that path-input operations (e.g. the
/// CLI's `validate`/`run`) resolve repo-root-relative paths exactly as a user
/// invoking the CLI from the repo root would.
fn repo_root() -> PathBuf {
    let mut p = workspace_root();
    p.pop(); // rust -> repo root
    p
}

/// Returns true if specgate-harness is being used from a local workspace
/// (path dependency) rather than from crates.io.
fn is_local_workspace() -> bool {
    let workspace = workspace_root();
    // If the workspace has a Cargo.toml with [workspace], we're local.
    // From crates.io, CARGO_MANIFEST_DIR is inside ~/.cargo/registry/src/...
    workspace.join("Cargo.toml").exists()
        && !workspace.to_string_lossy().contains(".cargo/registry")
        && !workspace.to_string_lossy().contains(".cargo\\registry")
}

fn scratch_for(stem: &str) -> PathBuf {
    let mut p = workspace_root();
    p.push("target");
    p.push("specgate-harness");
    p.push(stem);
    p
}

fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

/// Resolve the set of fixture source files needed to cover the operations the
/// cases require. Usually a single file (or one directory module), but
/// operations may be split across several separate top-level files — in that
/// case the minimal covering set is returned so the harness can merge them.
///
/// Prefers `src/<basename>.rs` when it exists. Otherwise scores each candidate
/// (top-level `.rs` file or directory module) by the required operations it
/// provides; if one candidate covers everything it wins, else a greedy set
/// cover over the remaining operations is returned.
fn resolve_fixture_sources(package_root: &Path, fixture_basename: &str, cases: &[&spec::Case]) -> Vec<PathBuf> {
    struct Candidate {
        repr: PathBuf,
        ops: BTreeSet<String>,
        score: usize,
    }

    let direct = package_root.join("src").join(format!("{fixture_basename}.rs"));
    if direct.exists() {
        return vec![direct];
    }

    let mut req_ops: BTreeSet<String> = BTreeSet::new();
    for case in cases {
        if !case.steps.is_empty() {
            for s in &case.steps {
                req_ops.insert(s.clone());
            }
        } else if let Some(op) = case.operation.as_deref() {
            req_ops.insert(op.to_string());
        }
    }

    let src_dir = package_root.join("src");
    let Ok(entries) = std::fs::read_dir(&src_dir) else {
        return Vec::new();
    };
    let mut cands: Vec<Candidate> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).map(ToString::to_string);
        // Directory module: merge all .rs files it contains and score together.
        // The representative path is the synthetic `src/<dirname>.rs`.
        let (text, repr) = if path.is_dir() {
            let Some(text) = merge_module_dir(&path) else { continue };
            let Some(dir_name) = stem.clone() else { continue };
            (text, src_dir.join(format!("{dir_name}.rs")))
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            (text, path.clone())
        } else {
            continue;
        };
        let annotated = scan(&text);
        let provided: BTreeSet<String> = req_ops.iter().filter(|o| annotated.operations.contains_key(*o)).cloned().collect();
        if provided.is_empty() {
            continue;
        }
        // Prefer sources whose provided operations actually wire (setups /
        // receivers resolve), and where the file stem matches the spec name.
        let provided_refs: Vec<&str> = provided.iter().map(String::as_str).collect();
        let wires = annotated.resolve_case(&provided_refs).is_ok();
        let mut score = provided.len() * 2 + usize::from(wires) * 100;
        if let Some(stem) = stem.as_deref()
            && fixture_basename.starts_with(stem)
            && stem.len() > 4
        {
            score += stem.len();
        }
        cands.push(Candidate {
            repr,
            ops: provided,
            score,
        });
    }

    // If a single candidate covers every required operation, take the best one.
    if let Some(full) = cands.iter().filter(|c| c.ops.len() == req_ops.len()).max_by_key(|c| c.score) {
        return vec![full.repr.clone()];
    }

    // Greedy set cover: take highest-scoring candidates until all required
    // operations are covered (or no candidate adds anything new).
    cands.sort_by_key(|c| std::cmp::Reverse(c.score));
    let mut covered: BTreeSet<String> = BTreeSet::new();
    let mut chosen: Vec<PathBuf> = Vec::new();
    for c in &cands {
        if c.ops.iter().any(|o| !covered.contains(o)) {
            covered.extend(c.ops.iter().cloned());
            chosen.push(c.repr.clone());
            if covered.len() == req_ops.len() {
                break;
            }
        }
    }
    chosen
}

/// Concatenate the source text of every `.rs` file under a module directory
/// (recursively), so that operations split across files are scanned together.
fn merge_module_dir(dir: &Path) -> Option<String> {
    let mut merged = String::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut found = false;
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                merged.push_str(&text);
                merged.push('\n');
                found = true;
            }
        }
    }
    found.then_some(merged)
}

/// Load the merged source text for a resolved fixture path. If the path is a
/// physical file, read it directly. Otherwise treat it as a synthetic module
/// file (`src/<name>.rs`) backed by a directory module (`src/<name>/`) and
/// merge all `.rs` files in that directory.
fn load_fixture_text(fixture_src: &Path) -> Result<String, String> {
    if fixture_src.exists() {
        return std::fs::read_to_string(fixture_src).map_err(|e| format!("source file unreadable: {} ({})", fixture_src.display(), e));
    }
    let dir = fixture_src.with_extension("");
    if dir.is_dir() {
        return merge_module_dir(&dir).ok_or_else(|| format!("module directory empty: {}", dir.display()));
    }
    Err(format!("source file not found: {}", fixture_src.display()))
}

// ---------------------------------------------------------------------------
// Shape check: every expected event key must be either `run`, a setup or
// operation input echo, an operation result/outcome/error/value, or one of
// the explicit `outputs` declared on the operation.
// ---------------------------------------------------------------------------

fn check_shape(spec: &spec::Spec, raw: &serde_yaml::Value) -> Option<String> {
    let ops_meta = ops_metadata(raw);
    for case in &spec.cases {
        let case_ops: Vec<&str> = if !case.steps.is_empty() {
            case.steps.iter().map(String::as_str).collect()
        } else if let Some(op) = case.operation.as_deref() {
            vec![op]
        } else {
            continue;
        };
        let mut allowed: BTreeSet<String> = BTreeSet::new();
        for op in &case_ops {
            if let Some(meta) = ops_meta.get(*op) {
                for inp in &meta.inputs {
                    allowed.insert(format!("{op}.{inp}"));
                }
                for out in &meta.outputs {
                    allowed.insert(out.clone());
                }
            }
            allowed.insert(format!("{op}.outcome"));
            allowed.insert(format!("{op}.result"));
            allowed.insert(format!("{op}.error"));
            allowed.insert(format!("{op}.value"));
            allowed.insert("$result".into());
            allowed.insert("$outcome".into());
            allowed.insert("$error".into());
            allowed.insert("$value".into());
            allowed.insert("$fault".into());
        }

        for entry in &case.expected {
            // Recursively collect leaf Event names from this assertion.
            let mut leaf_names: Vec<String> = Vec::new();
            collect_event_names(entry, &mut leaf_names);
            for k in &leaf_names {
                if allowed.contains(k) {
                    continue;
                }
                if case_ops.iter().any(|op| k.starts_with(&format!("{op}."))) {
                    continue;
                }
                // Schema check: a case must not assert on an output name the
                // operation never declares. If the operation declares any
                // outputs, a leaf that is neither in the allowed set nor a
                // `{op}.`-prefixed event (handled just above) is a schema
                // violation — a pre-flight harness Error, not a case failure.
                let strict_op = case_ops.iter().find(|op| ops_meta.get(**op).is_some_and(|m| !m.outputs.is_empty()));
                if let Some(op) = strict_op {
                    return Some(format!("expected event '{k}' is not a declared output of operation '{op}'"));
                }
            }
        }
    }
    None
}

fn collect_event_names(a: &Assertion, out: &mut Vec<String>) {
    match a {
        Assertion::Event { name, .. } => out.push(name.clone()),
        Assertion::Run { .. } => {}
        Assertion::Unordered { items } | Assertion::Anywhere { items } => {
            for it in items {
                collect_event_names(it, out);
            }
        }
    }
}

#[derive(Debug, Default)]
struct OpMeta {
    inputs: Vec<String>,
    outputs: Vec<String>,
}

fn ops_metadata(raw: &serde_yaml::Value) -> BTreeMap<String, OpMeta> {
    let mut out = BTreeMap::new();
    let Some(map) = raw.as_mapping() else { return out };
    let Some(serde_yaml::Value::Mapping(ops)) = map.get(serde_yaml::Value::String("operations".into())) else {
        return out;
    };
    for (k, v) in ops {
        let name = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let Some(body) = v.as_mapping() else { continue };
        let mut meta = OpMeta::default();
        if let Some(serde_yaml::Value::Mapping(inputs)) = body.get(serde_yaml::Value::String("inputs".into())) {
            for (ik, _) in inputs {
                if let Some(s) = ik.as_str() {
                    meta.inputs.push(s.to_string());
                }
            }
        }
        if let Some(serde_yaml::Value::Sequence(outs)) = body.get(serde_yaml::Value::String("outputs".into())) {
            for o in outs {
                // Per the schema, an output entry is either a bare string event
                // name (`count`) or an object mapping event name(s) to a type
                // reference. The value may be a scalar type (`i32`), a complex
                // `oneof:` block, or `{}`; we only need the event name(s) here.
                // Mappings are single-key by convention, but we register every
                // key so a multi-key entry can never silently drop an output.
                if let Some(s) = o.as_str() {
                    meta.outputs.push(s.to_string());
                } else if let Some(m) = o.as_mapping() {
                    for (k, _) in m {
                        if let Some(s) = k.as_str() {
                            meta.outputs.push(s.to_string());
                        }
                    }
                }
            }
        }
        out.insert(name, meta);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{CsOp, generate_csharp_program, ops_metadata, split_cs_params, yaml_to_csharp_literal};
    use crate::binding;
    use std::collections::BTreeMap;

    fn meta(yaml: &str) -> BTreeMap<String, super::OpMeta> {
        let raw: serde_yaml::Value = serde_yaml::from_str(yaml).expect("valid yaml");
        ops_metadata(&raw)
    }

    #[test]
    fn parses_bare_string_outputs() {
        let m = meta("operations:\n  increment:\n    outputs: [count]\n");
        assert_eq!(m["increment"].outputs, vec!["count".to_string()]);
    }

    #[test]
    fn parses_single_key_scalar_outputs() {
        let m = meta("operations:\n  divide:\n    inputs: { a: i32, b: i32 }\n    outputs:\n      - $result: i32\n");
        assert_eq!(m["divide"].outputs, vec!["$result".to_string()]);
        assert_eq!(m["divide"].inputs, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn parses_string_typed_collection_outputs() {
        // The value is a string type ref like `List<Point>`; only the event
        // name (`$result`) is registered as an output.
        let m = meta("operations:\n  get_points:\n    outputs:\n      - $result: \"List<Point>\"\n");
        assert_eq!(m["get_points"].outputs, vec!["$result".to_string()]);
    }

    #[test]
    fn parses_structured_collection_outputs() {
        // Collection output whose value is itself a `{type: list, items: ...}`
        // mapping. Only the outer event name is registered; the nested
        // type/items keys must NOT leak in as outputs.
        let yaml = "operations:\n  resolve:\n    outputs:\n      - entity_name: string\n      - key_properties:\n          type: list\n          items: string\n";
        let m = meta(yaml);
        assert_eq!(
            m["resolve"].outputs,
            vec!["entity_name".to_string(), "key_properties".to_string()],
            "nested type/items keys must not be registered as outputs"
        );
    }

    #[test]
    fn parses_map_outputs() {
        // `{type: map, keys, values}` — only `$result` is an output; the
        // nested keys/values/type keys must not leak.
        let yaml = "operations:\n  invert:\n    outputs:\n      - $result:\n          type: map\n          keys: string\n          values: string\n";
        let m = meta(yaml);
        assert_eq!(m["invert"].outputs, vec!["$result".to_string()]);
    }

    #[test]
    fn parses_set_outputs() {
        let yaml = "operations:\n  tags:\n    outputs:\n      - $result:\n          type: set\n          items: string\n";
        let m = meta(yaml);
        assert_eq!(m["tags"].outputs, vec!["$result".to_string()]);
    }

    #[test]
    fn parses_nested_list_of_structs_outputs() {
        // `{type: list, fields: {...}}` — the nested `fields` map (and its
        // own keys) must not be registered as outputs.
        let yaml = "operations:\n  cols:\n    outputs:\n      - $result:\n          type: list\n          fields:\n            name: string\n            nullable: string\n";
        let m = meta(yaml);
        assert_eq!(m["cols"].outputs, vec!["$result".to_string()]);
    }

    #[test]
    fn parses_enum_typed_outputs() {
        // Enum return type is a bare type-name string value; only `$result`
        // is registered.
        let m = meta("operations:\n  classify:\n    outputs:\n      - $result: Shape\n");
        assert_eq!(m["classify"].outputs, vec!["$result".to_string()]);
    }

    #[test]
    fn parses_complex_oneof_outputs() {
        let yaml =
            "operations:\n  run:\n    outputs:\n      - outcome:\n          oneof:\n            Complete: {}\n            Error: {}\n";
        let m = meta(yaml);
        assert_eq!(m["run"].outputs, vec!["outcome".to_string()]);
    }

    #[test]
    fn registers_every_key_of_a_multi_key_output_entry() {
        // The schema permits (though no spec uses) a multi-key output object;
        // every key must be registered so none is silently dropped.
        let m = meta("operations:\n  op:\n    outputs:\n      - { a: i32, b: i32 }\n");
        assert_eq!(m["op"].outputs, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn csharp_program_uses_declared_default_when_case_omits_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("value".to_string(), serde_yaml::Value::Number(5.into()));
        let case = super::spec::Case {
            name: "uses_default_factor".to_string(),
            target: None,
            operation: Some("scale".to_string()),
            steps: Vec::new(),
            step_inputs: Vec::new(),
            inputs,
            expected: Vec::new(),
            level: super::CaseLevel::Must,
            source: None,
        };
        let op = CsOp {
            op_name: "scale".to_string(),
            class_name: "ScaleOps".to_string(),
            method_name: "Scale".to_string(),
            params: vec![("value".to_string(), "int".to_string()), ("factor".to_string(), "int".to_string())],
            return_type: "int".to_string(),
        };
        let mut scale_defaults = BTreeMap::new();
        scale_defaults.insert("factor".to_string(), serde_yaml::Value::Number(2.into()));
        let mut defaults = BTreeMap::new();
        defaults.insert("scale".to_string(), scale_defaults);

        let program = generate_csharp_program(&[&case], &[op], &defaults);

        assert!(program.contains("int __value = 5;"));
        assert!(program.contains("int __factor = 2;"));
    }

    #[test]
    fn csharp_program_emits_fault_from_exception_message() {
        let case = super::spec::Case {
            name: "divide_by_zero_panics".to_string(),
            target: None,
            operation: Some("divide".to_string()),
            steps: Vec::new(),
            step_inputs: Vec::new(),
            inputs: BTreeMap::new(),
            expected: Vec::new(),
            level: super::CaseLevel::Must,
            source: None,
        };
        let op = CsOp {
            op_name: "divide".to_string(),
            class_name: "DivideOps".to_string(),
            method_name: "Divide".to_string(),
            params: Vec::new(),
            return_type: "int".to_string(),
        };
        let program = generate_csharp_program(&[&case], &[op], &BTreeMap::new());

        assert!(program.contains("catch (System.Exception __ex)"));
        assert!(program.contains("SpecGateRuntime.EmitEvent(\"$fault\", __ex.Message);"));
    }

    #[test]
    fn csharp_literal_escapes_control_characters() {
        let value = serde_yaml::Value::String("name: Customer\nid: 42".to_string());

        assert_eq!(yaml_to_csharp_literal(Some(&value), "string"), "\"name: Customer\\nid: 42\"");
    }

    #[test]
    fn csharp_literal_deserializes_complex_yaml_from_json() {
        let value: serde_yaml::Value = serde_yaml::from_str("{ dx: 1, dy: 2 }").expect("valid yaml");

        assert_eq!(
            yaml_to_csharp_literal(Some(&value), "Offset"),
            "FromJson<Offset>(\"{\\\"dx\\\":1,\\\"dy\\\":2}\")"
        );
    }

    #[test]
    fn csharp_param_split_keeps_generic_commas() {
        assert_eq!(
            split_cs_params("[SpecInput(\"m\")] Dictionary<string,string> m, int count"),
            vec!["[SpecInput(\"m\")] Dictionary<string,string> m", "int count"]
        );
    }

    // -----------------------------------------------------------------------
    // merge_target_results unit tests
    // -----------------------------------------------------------------------

    fn make_case_result(status: super::CaseStatus, traces: Vec<super::TraceEvent>) -> super::CaseResult {
        super::CaseResult {
            name: "test_case".to_string(),
            status,
            level: super::CaseLevel::Must,
            source: None,
            expected: Vec::new(),
            traces,
            target_failures: Vec::new(),
        }
    }

    fn ev_trace(name: &str, v: i64) -> super::TraceEvent {
        super::TraceEvent::Event {
            name: name.to_string(),
            value: super::Value::Integer(v),
        }
    }

    #[test]
    fn merge_both_fail_identical_traces_no_target_failures() {
        let traces = vec![ev_trace("result", 42)];
        let canonical = make_case_result(super::CaseStatus::Fail, traces.clone());
        let other = make_case_result(super::CaseStatus::Fail, traces.clone());
        let merged = super::merge_target_results(canonical, vec![("csharp".to_string(), other)]);
        assert!(
            merged.target_failures.is_empty(),
            "identical traces must not produce target_failures"
        );
        assert_eq!(merged.status, super::CaseStatus::Fail);
    }

    #[test]
    fn merge_non_canonical_different_traces_produces_target_failure() {
        let canonical_traces = vec![ev_trace("result", 1)];
        let other_traces = vec![ev_trace("result", 2), ev_trace("extra", 0)];
        let canonical = make_case_result(super::CaseStatus::Pass, canonical_traces.clone());
        let other = make_case_result(super::CaseStatus::Fail, other_traces.clone());
        let merged = super::merge_target_results(canonical, vec![("csharp".to_string(), other)]);
        assert_eq!(merged.target_failures.len(), 1, "diverging traces must produce one TargetFailure");
        assert_eq!(merged.target_failures[0].target, "csharp");
        assert_eq!(merged.target_failures[0].traces, other_traces);
        assert!(
            merged.target_failures[0].mismatch.contains("canonical"),
            "mismatch must describe canonical-vs-target divergence"
        );
        assert_eq!(merged.status, super::CaseStatus::Fail, "status must be Fail on divergence");
    }

    #[test]
    fn merge_both_pass_identical_traces_no_target_failures_status_pass() {
        let traces = vec![ev_trace("result", 7)];
        let canonical = make_case_result(super::CaseStatus::Pass, traces.clone());
        let other = make_case_result(super::CaseStatus::Pass, traces.clone());
        let merged = super::merge_target_results(canonical, vec![("csharp".to_string(), other)]);
        assert!(
            merged.target_failures.is_empty(),
            "identical passing traces must not produce target_failures"
        );
        assert_eq!(merged.status, super::CaseStatus::Pass);
    }

    fn make_target(framework: Option<&str>, pkg: &std::path::Path) -> binding::Target {
        binding::Target {
            package_root: pkg.to_path_buf(),
            command: None,
            runtime: binding::Runtime::Smol,
            framework: framework.map(String::from),
        }
    }

    #[test]
    fn extract_csproj_xml_tag_parses_target_framework() {
        let xml = "<Project><PropertyGroup><TargetFramework>net10.0</TargetFramework></PropertyGroup></Project>";
        assert_eq!(super::extract_csproj_xml_tag(xml, "TargetFramework"), Some("net10.0".to_string()));
    }

    #[test]
    fn extract_csproj_xml_tag_parses_target_frameworks_raw() {
        let xml = "<Project><PropertyGroup><TargetFrameworks>net8.0;net10.0</TargetFrameworks></PropertyGroup></Project>";
        // Returns the raw value; callers split on ';' to take the first item.
        assert_eq!(
            super::extract_csproj_xml_tag(xml, "TargetFrameworks"),
            Some("net8.0;net10.0".to_string())
        );
    }

    #[test]
    fn extract_csproj_xml_tag_absent_returns_none() {
        let xml = "<Project><PropertyGroup><OutputType>Exe</OutputType></PropertyGroup></Project>";
        assert_eq!(super::extract_csproj_xml_tag(xml, "TargetFramework"), None);
    }

    #[test]
    fn resolve_framework_uses_binding_framework_field() {
        let target = make_target(Some("net8.0"), std::path::Path::new("."));
        assert_eq!(super::resolve_framework_for_runner(&target), "net8.0");
    }

    #[test]
    fn resolve_framework_falls_back_from_netstandard_in_binding() {
        let target = make_target(Some("netstandard2.0"), std::path::Path::new("."));
        assert_eq!(super::resolve_framework_for_runner(&target), "net10.0");
    }

    #[test]
    fn resolve_framework_reads_target_framework_from_csproj() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("specgate-harness-unit-tests")
            .join(format!("fw_test_{}", ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("Test.csproj"),
            "<Project><PropertyGroup><TargetFramework>net9.0</TargetFramework></PropertyGroup></Project>",
        )
        .unwrap();
        let target = make_target(None, &scratch);
        assert_eq!(super::resolve_framework_for_runner(&target), "net9.0");
    }

    #[test]
    fn resolve_framework_reads_first_target_frameworks_from_csproj() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("specgate-harness-unit-tests")
            .join(format!("fw_multi_test_{}", ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("Multi.csproj"),
            "<Project><PropertyGroup><TargetFrameworks>net8.0;net10.0</TargetFrameworks></PropertyGroup></Project>",
        )
        .unwrap();
        let target = make_target(None, &scratch);
        assert_eq!(super::resolve_framework_for_runner(&target), "net8.0");
    }

    #[test]
    fn resolve_framework_falls_back_from_netstandard_in_csproj() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("specgate-harness-unit-tests")
            .join(format!("fw_ns_test_{}", ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("Lib.csproj"),
            "<Project><PropertyGroup><TargetFramework>netstandard2.0</TargetFramework></PropertyGroup></Project>",
        )
        .unwrap();
        let target = make_target(None, &scratch);
        assert_eq!(super::resolve_framework_for_runner(&target), "net10.0");
    }

    #[test]
    fn resolve_framework_defaults_to_net10_when_no_csproj() {
        let nonexistent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("nonexistent")
            .join("path")
            .join("that")
            .join("does")
            .join("not")
            .join("exist");
        let target = make_target(None, &nonexistent);
        assert_eq!(super::resolve_framework_for_runner(&target), "net10.0");
    }
}
