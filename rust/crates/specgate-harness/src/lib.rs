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
mod csharp_discovery;
#[doc(hidden)]
pub mod discovery;
mod match_traces;
pub(crate) mod scan;
mod spec;
mod types;

// Public API — what users need for run_spec() results
pub use types::{CaseLevel, CaseResult, CaseStatus, CoverageOutcome, CoverageReport, FileCoverage, RunOutcome, Source, TargetFailure};

// Public API — structural discovery (the schema counterpart to run_spec).
pub use discovery::{
    DiscoverOutcome, DiscoveredField, DiscoveredInput, DiscoveredOperation, DiscoveredSchema, DiscoveredType, DiscoveredVariant,
    TargetDiscovery, TargetOutcome, discover,
};

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

use serde::Deserialize;
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

    let fixture_basename = spec_basename(&path);

    // Shape check: spec-level event key validation.
    if let Some(reason) = check_shape(&parsed, &yaml_value, &bindings, &fixture_basename) {
        return err(reason);
    }

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
                run_csharp_group(target, &group_cases, &parsed, &scratch_dir).map(|r| (r, None))
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
        let mut merged = merge_target_results(canonical, others);
        if !merged.expected.is_empty() {
            merged.expected = reported_expected_for_case(&parsed.cases[i], &yaml_value);
        }
        results.push(merged);
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
    _fixture_basename: &str,
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

    let fixture_srcs = match run_discovery(&target.package_root)
        .and_then(|registry| metadata_fixture_sources(&target.package_root, &spec.name, group_cases, &registry))
    {
        Ok(Some(srcs)) => srcs,
        Ok(None) => {
            if let Some(results) = short_circuit_non_must(group_cases, None) {
                return Ok((results, None));
            }
            let required_ops = required_operations(group_cases);
            if spec.name == "fixture.compile_error" && required_ops.contains("broken") {
                let compile_error_path = target.package_root.join("src").join("engine").join("compile_error.rs");
                if compile_error_path.exists() {
                    vec![compile_error_path]
                } else {
                    let op = required_ops.iter().next().map_or("<unknown>", String::as_str);
                    return Err(format!("operation '{op}' not found for component '{}'", spec.name));
                }
            } else {
                let op = required_ops.iter().next().map_or("<unknown>", String::as_str);
                return Err(format!("operation '{op}' not found for component '{}'", spec.name));
            }
        }
        Err(reason) => return Err(reason),
    };
    if fixture_srcs.is_empty() {
        if let Some(results) = short_circuit_non_must(group_cases, None) {
            return Ok((results, None));
        }
        let required_ops = required_operations(group_cases);
        let op = required_ops.iter().next().map_or("<unknown>", String::as_str);
        return Err(format!("operation '{op}' not found for component '{}'", spec.name));
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
        if !codegen::module_publicly_linkable(&target.package_root, src) {
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
#[derive(Clone)]
struct CsOp {
    op_name: String,
    component: Option<String>,
    source_path: PathBuf,
    /// Fully-qualified class name for generated calls.
    class_name: String,
    /// Bare declaring class name, used for setup/receiver type matching.
    method_of: String,
    method_name: String,
    /// `(parameter_name, cs_type)`
    params: Vec<(String, String)>,
    return_type: String,
    return_nullable: bool,
    exception_types: Option<Vec<String>>,
    is_static: bool,
}

/// Metadata about a C# setup found by scanning source files.
#[derive(Clone)]
struct CsSetup {
    operation: String,
    component: Option<String>,
    source_path: PathBuf,
    fills: Option<String>,
    class_name: String,
    method_name: String,
    params: Vec<(String, String)>,
    return_type: String,
}

#[derive(Clone)]
struct CsSetupBinding {
    setup: CsSetup,
    var: String,
    target: CsSetupTarget,
}

#[derive(Clone, PartialEq, Eq)]
enum CsSetupTarget {
    Receiver,
    Param(String),
    SideEffect,
}

/// Scan all `.cs` files under `package_root` recursively for `[SpecOperation("name")]`
/// annotations and extract the annotated method's class, name, params, and return type.
fn scan_csharp(package_root: &Path) -> (Vec<CsOp>, Vec<CsSetup>) {
    let mut ops = Vec::new();
    let mut setups = Vec::new();
    scan_csharp_dir(package_root, &mut ops, &mut setups);
    (ops, setups)
}

fn scan_csharp_dir(dir: &Path, ops: &mut Vec<CsOp>, setups: &mut Vec<CsSetup>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            scan_csharp_dir(&p, ops, setups);
        } else if p.extension().and_then(|e| e.to_str()) == Some("cs")
            && let Ok(text) = std::fs::read_to_string(&p)
        {
            scan_csharp_file(&text, &p, ops, setups);
        }
    }
}

fn scan_csharp_file(text: &str, source_path: &Path, ops: &mut Vec<CsOp>, setups: &mut Vec<CsSetup>) {
    let mut current_class: Option<String> = None;
    let mut current_namespace: Option<String> = None;
    let mut pending_attrs: Vec<CsPendingAttr> = Vec::new();
    let mut pending_exception_types: Option<Vec<String>> = None;
    let mut pending_sig = String::new();

    for line in text.lines() {
        let trimmed = line.trim();

        if let Some(ns) = extract_cs_namespace(trimmed) {
            current_namespace = Some(ns);
        }

        // Detect class declaration: look for keyword "class" followed by the name.
        if let Some(class_name) = extract_cs_class(trimmed) {
            current_class = Some(class_name);
        }

        // Detect [SpecOperation("name")] / [SpecSetup("name", Fills = "...")].
        if let Some((op_name, component)) = extract_spec_operation_attr(trimmed) {
            pending_attrs.push(CsPendingAttr::Operation(op_name, component));
            pending_sig.clear();
            continue;
        }
        if let Some((operation, fills, component)) = extract_spec_setup_attr(trimmed) {
            pending_attrs.push(CsPendingAttr::Setup {
                operation,
                fills,
                component,
            });
            pending_sig.clear();
            continue;
        }
        if let Some(exception_types) = extract_spec_exception_attr(trimmed) {
            pending_exception_types = Some(exception_types);
            pending_sig.clear();
            continue;
        }

        // If we have pending spec attributes, try to parse the next method signature.
        if !pending_attrs.is_empty() && !trimmed.is_empty() {
            if !pending_sig.is_empty() {
                pending_sig.push(' ');
            }
            pending_sig.push_str(trimmed);
        }
        if !pending_attrs.is_empty()
            && let Some((method_name, params, return_type, is_static)) = extract_cs_method_sig(&pending_sig)
        {
            let bare_class = current_class.clone().unwrap_or_default();
            let class_name = qualify_cs_class(current_namespace.as_deref(), &bare_class);
            for attr in pending_attrs.drain(..) {
                match attr {
                    CsPendingAttr::Operation(op_name, component) => ops.push(CsOp {
                        op_name,
                        component,
                        source_path: source_path.to_path_buf(),
                        class_name: class_name.clone(),
                        method_of: bare_class.clone(),
                        method_name: method_name.clone(),
                        params: params.clone(),
                        return_type: return_type.clone(),
                        return_nullable: is_nullable_cs_type(&return_type),
                        exception_types: pending_exception_types.clone(),
                        is_static,
                    }),
                    CsPendingAttr::Setup {
                        operation,
                        fills,
                        component,
                    } => setups.push(CsSetup {
                        operation,
                        component,
                        source_path: source_path.to_path_buf(),
                        fills,
                        class_name: class_name.clone(),
                        method_name: method_name.clone(),
                        params: params.clone(),
                        return_type: return_type.clone(),
                    }),
                }
            }
            pending_exception_types = None;
            pending_sig.clear();
        } else if !pending_attrs.is_empty()
            && !trimmed.starts_with('[')
            && !trimmed.is_empty()
            && (trimmed.ends_with(';') || trimmed.ends_with('{'))
            && !pending_sig.contains('(')
        {
            pending_attrs.clear();
            pending_exception_types = None;
            pending_sig.clear();
        }
    }
}

enum CsPendingAttr {
    Operation(String, Option<String>),
    Setup {
        operation: String,
        fills: Option<String>,
        component: Option<String>,
    },
}

fn extract_cs_namespace(line: &str) -> Option<String> {
    let rest = line.strip_prefix("namespace ")?;
    let end = rest.find([';', '{']).unwrap_or(rest.len());
    let name = rest[..end].trim();
    if name.is_empty() { None } else { Some(name.to_string()) }
}

fn qualify_cs_class(namespace: Option<&str>, class_name: &str) -> String {
    match namespace {
        Some(ns) if !ns.is_empty() => format!("{ns}.{class_name}"),
        _ => class_name.to_string(),
    }
}

fn extract_spec_setup_attr(line: &str) -> Option<(String, Option<String>, Option<String>)> {
    let s = line.trim();
    let rest = s.strip_prefix("[SpecSetup(\"")?;
    let operation = rest.split('"').next()?.to_string();
    let fills = rest
        .split("Fills")
        .nth(1)
        .and_then(|r| r.split('"').nth(1))
        .map(ToString::to_string);
    let component = rest.split("Spec").nth(1).and_then(|r| r.split('"').nth(1)).map(ToString::to_string);
    Some((operation, fills, component))
}

#[allow(clippy::type_complexity)]
fn extract_cs_method_sig(line: &str) -> Option<(String, Vec<(String, String)>, String, bool)> {
    let paren_open = line.find('(')?;
    let paren_close = find_matching_paren(line, paren_open)?;

    let before = line[..paren_open].trim();
    let member = parse_csharp_member_prefix(before)?;
    let method_name = member.name;
    let return_type = member.ty;
    let is_static = member.modifiers.iter().any(|m| m == "static");

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

    Some((method_name, params, return_type, is_static))
}

fn resolve_csharp_case(cs_ops: &[CsOp], cs_setups: &[CsSetup], ops: &[&str]) -> Result<Vec<CsSetupBinding>, String> {
    let mut distinct: Vec<&str> = Vec::new();
    for &o in ops {
        if !distinct.contains(&o) {
            distinct.push(o);
        }
    }

    let mut pool: Vec<&CsSetup> = Vec::new();
    for o in &distinct {
        pool.extend(cs_setups.iter().filter(|s| s.operation == *o));
    }

    let mut bindings = Vec::new();
    let mut used = vec![false; pool.len()];
    let mut counter = 0usize;
    let mut new_var = || {
        let var = format!("__sg_setup{counter}");
        counter += 1;
        var
    };

    let receiver_ty = distinct
        .iter()
        .filter_map(|o| cs_ops.iter().find(|op| op.op_name == *o))
        .find(|op| !op.is_static)
        .map(|op| op.method_of.clone());
    if let Some(receiver_ty) = receiver_ty {
        let cands: Vec<usize> = pool
            .iter()
            .enumerate()
            .filter(|(i, s)| !used[*i] && s.fills.is_none() && bare_cs_type(&s.return_type) == receiver_ty)
            .map(|(i, _)| i)
            .collect();
        match cands.len() {
            0 => {
                let op = distinct.first().copied().unwrap_or("");
                return Err(format!(
                    "C# operation '{op}' is a method on '{receiver_ty}' but no [SpecSetup(\"{op}\")] returns '{receiver_ty}' to construct the receiver"
                ));
            }
            1 => {
                let i = cands[0];
                used[i] = true;
                bindings.push(CsSetupBinding {
                    setup: pool[i].clone(),
                    var: new_var(),
                    target: CsSetupTarget::Receiver,
                });
            }
            n => return Err(format!("{n} C# setups return '{receiver_ty}' for the receiver")),
        }
    }

    for o in &distinct {
        let Some(decl) = cs_ops.iter().find(|op| op.op_name == *o) else {
            continue;
        };
        for (param, ty) in &decl.params {
            if bindings.iter().any(|b| matches!(&b.target, CsSetupTarget::Param(n) if n == param)) {
                continue;
            }
            let bare_ty = bare_cs_type(ty);
            let pinned: Vec<usize> = pool
                .iter()
                .enumerate()
                .filter(|(i, s)| !used[*i] && s.fills.as_deref() == Some(param.as_str()) && bare_cs_type(&s.return_type) == bare_ty)
                .map(|(i, _)| i)
                .collect();
            if pinned.len() > 1 {
                return Err(format!("C# operation '{o}': multiple setups fill parameter '{param}'"));
            }
            if let Some(&i) = pinned.first() {
                used[i] = true;
                bindings.push(CsSetupBinding {
                    setup: pool[i].clone(),
                    var: new_var(),
                    target: CsSetupTarget::Param(param.clone()),
                });
                continue;
            }
            let typed: Vec<usize> = pool
                .iter()
                .enumerate()
                .filter(|(i, s)| !used[*i] && s.fills.is_none() && bare_cs_type(&s.return_type) == bare_ty)
                .map(|(i, _)| i)
                .collect();
            if typed.is_empty() {
                continue;
            }
            let same_type_params = decl.params.iter().filter(|(_, t)| bare_cs_type(t) == bare_ty).count();
            if typed.len() == 1 && same_type_params == 1 {
                let i = typed[0];
                used[i] = true;
                bindings.push(CsSetupBinding {
                    setup: pool[i].clone(),
                    var: new_var(),
                    target: CsSetupTarget::Param(param.clone()),
                });
            } else {
                return Err(format!(
                    "C# operation '{o}' has {same_type_params} parameters of type '{bare_ty}' and {} setups producing it; pin each setup with Fills",
                    typed.len()
                ));
            }
        }
    }

    for (i, setup) in pool.iter().enumerate() {
        if used[i] {
            continue;
        }
        if let Some(fills) = &setup.fills {
            let has_param = distinct
                .iter()
                .filter_map(|o| cs_ops.iter().find(|op| op.op_name == *o))
                .any(|op| op.params.iter().any(|(param, _)| param == fills));
            if has_param {
                return Err(format!(
                    "C# setup fills '{fills}' but its return type '{}' does not match parameter '{fills}'",
                    setup.return_type.trim()
                ));
            }
            return Err(format!(
                "C# setup fills '{fills}' but no operation in the case has a parameter '{fills}'"
            ));
        }
        bindings.push(CsSetupBinding {
            setup: (*setup).clone(),
            var: new_var(),
            target: CsSetupTarget::SideEffect,
        });
    }

    Ok(bindings)
}

fn bare_cs_type(ty: &str) -> String {
    let mut out = ty.trim();
    for prefix in ["ref ", "out ", "in "] {
        if let Some(rest) = out.strip_prefix(prefix) {
            out = rest.trim_start();
        }
    }
    out.trim_end_matches('?').rsplit('.').next().unwrap_or(out).trim().to_string()
}

fn extract_cs_class(line: &str) -> Option<String> {
    let words: Vec<&str> = line.split_whitespace().collect();
    let idx = words.iter().position(|&w| w == "class")?;
    let raw = words.get(idx + 1)?;
    // Strip trailing '{', ':', '<T>' etc.
    let name: String = raw.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    if name.is_empty() { None } else { Some(name) }
}

fn extract_spec_operation_attr(line: &str) -> Option<(String, Option<String>)> {
    let s = line.trim();
    let rest = s.strip_prefix("[SpecOperation(\"")?;
    let name = rest.split('"').next()?.to_string();
    let component = rest.split("Spec").nth(1).and_then(|r| r.split('"').nth(1)).map(ToString::to_string);
    Some((name, component))
}

fn extract_spec_exception_attr(line: &str) -> Option<Vec<String>> {
    let s = line.trim();
    if !(s.starts_with("[SpecException") || s.starts_with("[SpecGate.Annotations.SpecException")) {
        return None;
    }
    let Some(open) = s.find('(') else {
        return Some(Vec::new());
    };
    let close = find_matching_paren(s, open).unwrap_or_else(|| s.rfind(')').unwrap_or(open));
    let args = s[open + 1..close].trim();
    if args.is_empty() {
        return Some(Vec::new());
    }

    let mut types = Vec::new();
    for arg in split_cs_params(args) {
        let arg = arg.trim();
        let ty = arg
            .strip_prefix("typeof")
            .and_then(|rest| {
                let rest = rest.trim_start();
                let open = rest.find('(')?;
                let close = find_matching_paren(rest, open)?;
                Some(rest[open + 1..close].trim())
            })
            .unwrap_or(arg)
            .trim_start_matches("global::");
        let simple = ty.rsplit('.').next().unwrap_or(ty).trim();
        if !simple.is_empty() && !types.iter().any(|existing| existing == simple) {
            types.push(simple.to_string());
        }
    }
    Some(types)
}

fn is_nullable_cs_type(cs_type: &str) -> bool {
    cs_type.trim().ends_with('?')
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

struct CsMemberPrefix {
    modifiers: Vec<String>,
    ty: String,
    name: String,
}

fn parse_csharp_member_prefix(prefix: &str) -> Option<CsMemberPrefix> {
    let prefix = prefix.trim();
    let name_start = find_last_csharp_ident_start(prefix)?;
    let name = prefix[name_start..].trim().to_string();
    let before_name = prefix[..name_start].trim_end();
    if before_name.is_empty() || name.is_empty() {
        return None;
    }

    let mut cursor = 0usize;
    let mut modifiers = Vec::new();
    loop {
        cursor += before_name[cursor..].len() - before_name[cursor..].trim_start().len();
        let rest = &before_name[cursor..];
        let Some((token, next)) = read_csharp_ident(rest) else {
            break;
        };
        if !is_csharp_member_modifier(token) {
            break;
        }
        modifiers.push(token.to_string());
        cursor += next;
    }

    let ty = before_name[cursor..].trim().to_string();
    if ty.is_empty() {
        return None;
    }

    Some(CsMemberPrefix { modifiers, ty, name })
}

fn find_last_csharp_ident_start(s: &str) -> Option<usize> {
    let mut end = None;
    for (idx, ch) in s.char_indices().rev() {
        if end.is_none() {
            if ch.is_whitespace() {
                continue;
            }
            if is_csharp_ident_continue(ch) {
                end = Some(idx + ch.len_utf8());
                continue;
            }
            return None;
        }
        if !is_csharp_ident_continue(ch) {
            return Some(if ch == '@' { idx } else { idx + ch.len_utf8() });
        }
    }
    end.map(|_| 0)
}

fn read_csharp_ident(s: &str) -> Option<(&str, usize)> {
    let mut iter = s.char_indices();
    let (first_idx, first) = iter.next()?;
    debug_assert_eq!(first_idx, 0);
    if !is_csharp_ident_start(first) {
        return None;
    }
    let mut end = first.len_utf8();
    for (idx, ch) in iter {
        if !is_csharp_ident_continue(ch) {
            break;
        }
        end = idx + ch.len_utf8();
    }
    Some((&s[..end], end))
}

fn is_csharp_ident_start(ch: char) -> bool {
    ch == '_' || ch == '@' || ch.is_alphabetic()
}

fn is_csharp_ident_continue(ch: char) -> bool {
    ch == '_' || ch.is_alphanumeric()
}

fn is_csharp_member_modifier(token: &str) -> bool {
    matches!(
        token,
        "public"
            | "private"
            | "protected"
            | "internal"
            | "static"
            | "virtual"
            | "override"
            | "abstract"
            | "sealed"
            | "new"
            | "readonly"
            | "unsafe"
            | "extern"
            | "required"
            | "partial"
            | "async"
    )
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
    if is_cs_option_type(param_type) || is_nullable_cs_type(param_type) {
        let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
        return format!("FromSpecInput<{}>({})", param_type.trim(), csharp_string_literal(&json));
    }
    if matches!(v, serde_yaml::Value::Null) {
        let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
        return format!("FromSpecInput<{}>({})", param_type.trim(), csharp_string_literal(&json));
    }
    if v.as_mapping().is_some() || v.as_sequence().is_some() {
        let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
        return format!("FromSpecInput<{}>({})", param_type.trim(), csharp_string_literal(&json));
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
        if !matches!(param_type.trim(), "string" | "String") {
            let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
            return format!("FromSpecInput<{}>({})", param_type.trim(), csharp_string_literal(&json));
        }
        return csharp_string_literal(s);
    }
    "default!".to_string()
}

fn yaml_to_csharp_echo_literal(val: Option<&serde_yaml::Value>) -> String {
    let Some(v) = val else { return "(object?)null".to_string() };
    if matches!(v, serde_yaml::Value::Null) {
        return "(object?)null".to_string();
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
    let json = serde_json::to_string(v).unwrap_or_else(|_| "null".to_string());
    format!("FromSpecInput<object>({})", csharp_string_literal(&json))
}

fn is_cs_option_type(cs_type: &str) -> bool {
    cs_type.trim_start().starts_with("Option<")
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

fn escape_xml_text(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
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
    cs_setups: &[CsSetup],
    op_input_defaults: &BTreeMap<String, BTreeMap<String, serde_yaml::Value>>,
) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str("using SpecGate.Annotations;\n");
    out.push_str("using SpecGate.Runtime;\n");
    out.push_str("using System;\n");
    out.push_str("using System.Collections;\n");
    out.push_str("using SpecGateFixtures;\n");
    let mut namespaces = BTreeSet::new();
    for case in cases {
        let case_ops = csharp_case_ops(case);
        for op_name in &case_ops {
            if let Some(op) = cs_ops.iter().find(|op| op.op_name == *op_name)
                && let Some((ns, _)) = op.class_name.rsplit_once('.')
                && ns != "SpecGateFixtures"
            {
                namespaces.insert(ns.to_string());
            }
        }
        for binding in resolve_csharp_case(cs_ops, cs_setups, &case_ops)? {
            if let Some((ns, _)) = binding.setup.class_name.rsplit_once('.')
                && ns != "SpecGateFixtures"
            {
                namespaces.insert(ns.to_string());
            }
        }
    }
    for ns in namespaces {
        writeln!(out, "using {ns};").expect("fmt");
    }
    out.push_str("using System.Collections.Generic;\n");
    out.push_str("using System.IO;\n");
    out.push_str("using System.Linq;\n");
    out.push_str("using System.Reflection;\n");
    out.push_str("using System.Text;\n\n");
    out.push_str("using System.Text.Json;\n\n");
    out.push_str("var all = new SortedDictionary<string, string>();\n");

    for case in cases {
        let case_ops = csharp_case_ops(case);
        let bindings = resolve_csharp_case(cs_ops, cs_setups, &case_ops)?;
        writeln!(out, "// case: {}", case.name).expect("fmt");
        out.push_str("{\n");
        out.push_str("    SpecGateRuntime.Reset();\n");
        emit_csharp_mock_tables(&mut out, &case.inputs);

        for binding in &bindings {
            let args = render_csharp_construct_args(&binding.setup.params, &binding.target, &case.inputs);
            match &binding.target {
                CsSetupTarget::SideEffect => {
                    writeln!(out, "    _ = {}.{}({args});", binding.setup.class_name, binding.setup.method_name).expect("fmt");
                }
                CsSetupTarget::Receiver | CsSetupTarget::Param(_) => {
                    writeln!(
                        out,
                        "    var {} = {}.{}({args});",
                        binding.var, binding.setup.class_name, binding.setup.method_name
                    )
                    .expect("fmt");
                    let prefix = match &binding.target {
                        CsSetupTarget::Receiver => "null".to_string(),
                        CsSetupTarget::Param(param) => csharp_string_literal(param),
                        CsSetupTarget::SideEffect => unreachable!(),
                    };
                    writeln!(out, "    SpecGateRuntime.RegisterObject({}, {prefix});", binding.var).expect("fmt");
                    writeln!(out, "    SpecGateRuntime.EmitFields({}, {prefix});", binding.var).expect("fmt");
                }
            }
        }

        let is_steps = !case.steps.is_empty();
        for (step_idx, op_name) in case_ops.iter().enumerate() {
            let Some(cs_op) = cs_ops.iter().find(|o| o.op_name == *op_name) else {
                continue;
            };
            let op_defaults = op_input_defaults.get(*op_name);
            let inputs = if is_steps {
                case.step_inputs.get(step_idx).filter(|m| !m.is_empty()).unwrap_or(&case.inputs)
            } else {
                &case.inputs
            };
            writeln!(out, "    SpecGateRuntime.EmitRun(\"{op_name}\");").expect("fmt");
            let args = render_csharp_op_args(&mut out, cs_op, inputs, &bindings, op_defaults, step_idx);
            let call = render_csharp_op_call(cs_op, &bindings, &args);
            out.push_str("    SpecGateRuntime.SuppressNextOperationInstrumentation();\n");
            out.push_str("    try {\n");
            let return_kind = csharp_return_kind(&cs_op.return_type)?;
            emit_csharp_call_and_result(&mut out, cs_op, &call, &return_kind, step_idx);
            emit_csharp_catches(&mut out, cs_op);
            out.push_str("    SpecGateRuntime.ClearOperationInstrumentationSuppression();\n");
        }
        writeln!(out, "    all[\"{}\"] = SpecGateRuntime.GetTracesJson();", case.name).expect("fmt");
        out.push_str("}\n");
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
    out.push_str(csharp_materialization_helpers());
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
    Ok(out)
}

fn emit_csharp_call_and_result(out: &mut String, cs_op: &CsOp, call: &str, return_kind: &CsReturnKind, step_idx: usize) {
    use std::fmt::Write as _;
    match return_kind {
        CsReturnKind::Void => {
            writeln!(out, "        {call};").expect("fmt");
        }
        CsReturnKind::Value(result_type) => {
            writeln!(out, "        {result_type} __sg_result_{step_idx} = {call};").expect("fmt");
            emit_csharp_result(out, cs_op, step_idx);
        }
        CsReturnKind::AsyncVoid => {
            writeln!(out, "        await {call};").expect("fmt");
        }
        CsReturnKind::AsyncValue(result_type) => {
            writeln!(out, "        {result_type} __sg_result_{step_idx} = await {call};").expect("fmt");
            emit_csharp_result(out, cs_op, step_idx);
        }
    }
}

fn emit_csharp_result(out: &mut String, cs_op: &CsOp, step_idx: usize) {
    use std::fmt::Write as _;
    let var = format!("__sg_result_{step_idx}");
    if cs_op.exception_types.is_some() {
        writeln!(out, "        SpecGateRuntime.EmitTaggedResult(\"Ok\", {var});").expect("fmt");
    } else if cs_op.return_nullable {
        writeln!(out, "        SpecGateRuntime.EmitOptionResult({var});").expect("fmt");
    } else {
        writeln!(out, "        SpecGateRuntime.EmitResult({var});").expect("fmt");
    }
}

fn emit_csharp_catches(out: &mut String, cs_op: &CsOp) {
    use std::fmt::Write as _;
    match &cs_op.exception_types {
        Some(types) if types.is_empty() => {
            out.push_str("    } catch (System.Exception __ex) {\n");
            out.push_str("        SpecGateRuntime.EmitTaggedResult(\"Err\", __ex.Message);\n");
            out.push_str("    }\n");
        }
        Some(types) => {
            for ty in types {
                writeln!(out, "    }} catch ({ty} __ex) {{").expect("fmt");
                out.push_str("        SpecGateRuntime.EmitTaggedResult(\"Err\", __ex.Message);\n");
            }
            out.push_str("    } catch (System.Exception __ex) {\n");
            out.push_str("        SpecGateRuntime.EmitEvent(\"$fault\", __ex.Message);\n");
            out.push_str("    }\n");
        }
        None => {
            out.push_str("    } catch (System.Exception __ex) {\n");
            out.push_str("        SpecGateRuntime.EmitEvent(\"$fault\", __ex.Message);\n");
            out.push_str("    }\n");
        }
    }
}

fn csharp_case_ops(case: &spec::Case) -> Vec<&str> {
    if case.steps.is_empty() {
        case.operation.as_deref().into_iter().collect()
    } else {
        case.steps.iter().map(String::as_str).collect()
    }
}

enum CsReturnKind {
    Void,
    Value(String),
    AsyncVoid,
    AsyncValue(String),
}

fn csharp_return_kind(return_type: &str) -> Result<CsReturnKind, String> {
    let ty = return_type.trim();
    if ty == "void" {
        return Ok(CsReturnKind::Void);
    }

    if let Some(kind) = csharp_async_return_kind(ty)? {
        return Ok(kind);
    }

    Ok(CsReturnKind::Value(ty.to_string()))
}

fn csharp_async_return_kind(return_type: &str) -> Result<Option<CsReturnKind>, String> {
    let ty = return_type.trim();
    if bare_cs_type(ty) == "Task" || bare_cs_type(ty) == "ValueTask" {
        return Ok(Some(CsReturnKind::AsyncVoid));
    }

    let Some(open) = ty.find('<') else {
        return if csharp_type_mentions_task(ty) {
            Err(format!("unsupported C# async return type '{ty}'"))
        } else {
            Ok(None)
        };
    };
    let Some(close) = find_matching_angle(ty, open) else {
        return Err(format!("unsupported C# async return type '{ty}'"));
    };
    if !ty[close + 1..].trim().is_empty() {
        return if csharp_type_mentions_task(ty) {
            Err(format!("unsupported C# async return type '{ty}'"))
        } else {
            Ok(None)
        };
    }

    let outer = bare_cs_type(&ty[..open]);
    if outer != "Task" && outer != "ValueTask" {
        return Ok(None);
    }

    let inner = ty[open + 1..close].trim();
    if inner.is_empty() || split_cs_params(inner).len() != 1 {
        return Err(format!("unsupported C# async return type '{ty}'"));
    }
    Ok(Some(CsReturnKind::AsyncValue(inner.to_string())))
}

fn csharp_type_mentions_task(ty: &str) -> bool {
    ty.split(|ch: char| !(ch.is_alphanumeric() || ch == '_' || ch == '.'))
        .any(|part| matches!(bare_cs_type(part).as_str(), "Task" | "ValueTask"))
}

fn find_matching_angle(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in s.char_indices().skip_while(|(idx, _)| *idx < open) {
        match ch {
            '<' => depth += 1,
            '>' => {
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

fn csharp_materialization_helpers() -> &'static str {
    r#"static T FromSpecInput<T>(string json) => (T)FromSpecInputValue(typeof(T), JsonSerializer.Deserialize<JsonElement>(json))!;

static object? FromSpecInputValue(Type targetType, JsonElement value) {
    if (targetType == typeof(string)) return value.ValueKind == JsonValueKind.Null ? null : value.GetString();
    if (targetType == typeof(int)) return value.GetInt32();
    if (targetType == typeof(long)) return value.GetInt64();
    if (targetType == typeof(bool)) return value.GetBoolean();
    if (targetType == typeof(double)) return value.GetDouble();
    if (targetType == typeof(float)) return value.GetSingle();

    Type? nullableInner = Nullable.GetUnderlyingType(targetType);
    if (nullableInner is not null) {
        return value.ValueKind == JsonValueKind.Null ? null : FromSpecInputValue(nullableInner, value);
    }

    if (targetType.IsGenericType && targetType.GetGenericTypeDefinition() == typeof(Option<>)) {
        if (value.ValueKind == JsonValueKind.Null) {
            return targetType.GetMethod("None", BindingFlags.Public | BindingFlags.Static)!.Invoke(null, Array.Empty<object>());
        }

        Type innerType = targetType.GetGenericArguments()[0];
        object? inner = FromSpecInputValue(innerType, value);
        return targetType.GetMethod("Some", BindingFlags.Public | BindingFlags.Static)!.Invoke(null, new[] { inner });
    }

    if (targetType.IsGenericType && targetType.GetGenericTypeDefinition() == typeof(List<>)) {
        Type itemType = targetType.GetGenericArguments()[0];
        var list = (IList)Activator.CreateInstance(targetType)!;
        foreach (JsonElement item in value.EnumerateArray()) {
            list.Add(FromSpecInputValue(itemType, item));
        }
        return list;
    }

    if (targetType.IsGenericType && targetType.GetGenericTypeDefinition() == typeof(Dictionary<,>)) {
        Type[] args = targetType.GetGenericArguments();
        var dict = (IDictionary)Activator.CreateInstance(targetType)!;
        foreach (JsonProperty property in value.EnumerateObject()) {
            object key = args[0] == typeof(string) ? property.Name : Convert.ChangeType(property.Name, args[0], System.Globalization.CultureInfo.InvariantCulture);
            dict.Add(key, FromSpecInputValue(args[1], property.Value));
        }
        return dict;
    }

    if (targetType.IsAbstract) {
        string tag;
        JsonElement payload;
        if (value.ValueKind == JsonValueKind.String) {
            tag = value.GetString()!;
            payload = default;
        } else {
            JsonProperty property = value.EnumerateObject().Single();
            tag = property.Name;
            payload = property.Value;
        }

        Type variantType = targetType.Assembly.GetTypes()
            .Where(t => !t.IsAbstract && targetType.IsAssignableFrom(t))
            .Single(t => SpecEventName(t) == tag || t.Name == tag);
        return payload.ValueKind == JsonValueKind.Undefined || payload.ValueKind == JsonValueKind.Null
            ? Activator.CreateInstance(variantType)
            : FromSpecInputValue(variantType, payload);
    }

    if (value.ValueKind == JsonValueKind.Object) {
        object instance = Activator.CreateInstance(targetType)!;
        foreach (PropertyInfo property in targetType.GetProperties(BindingFlags.Instance | BindingFlags.Public)) {
            if (!property.CanWrite || property.GetIndexParameters().Length != 0) continue;
            if (TryGetProperty(value, SpecMemberName(property), out JsonElement propertyValue)
                || TryGetProperty(value, property.Name, out propertyValue)) {
                property.SetValue(instance, FromSpecInputValue(property.PropertyType, propertyValue));
            }
        }
        foreach (FieldInfo field in targetType.GetFields(BindingFlags.Instance | BindingFlags.Public)) {
            if (TryGetProperty(value, SpecMemberName(field), out JsonElement fieldValue)
                || TryGetProperty(value, field.Name, out fieldValue)) {
                field.SetValue(instance, FromSpecInputValue(field.FieldType, fieldValue));
            }
        }
        return instance;
    }

    return JsonSerializer.Deserialize(value.GetRawText(), targetType, new JsonSerializerOptions { IncludeFields = true, PropertyNameCaseInsensitive = true });
}

static bool TryGetProperty(JsonElement obj, string name, out JsonElement value) {
    foreach (JsonProperty property in obj.EnumerateObject()) {
        if (property.Name == name || string.Equals(property.Name, name, StringComparison.OrdinalIgnoreCase)) {
            value = property.Value;
            return true;
        }
    }
    value = default;
    return false;
}

static string SpecMemberName(MemberInfo member) {
    Attribute? attr = member.GetCustomAttributes(false)
        .OfType<Attribute>()
        .FirstOrDefault(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");
    return (attr?.GetType().GetProperty("Name")?.GetValue(attr) as string) ?? member.Name;
}

static string SpecEventName(Type type) {
    Attribute? attr = type.GetCustomAttributes(false)
        .OfType<Attribute>()
        .FirstOrDefault(a => a.GetType().FullName == "SpecGate.Annotations.SpecEventAttribute");
    return (attr?.GetType().GetProperty("Name")?.GetValue(attr) as string) ?? type.Name;
}

"#
}

fn render_csharp_construct_args(
    params: &[(String, String)],
    target: &CsSetupTarget,
    inputs: &BTreeMap<String, serde_yaml::Value>,
) -> String {
    let role = match target {
        CsSetupTarget::Param(param) => Some(param.as_str()),
        CsSetupTarget::Receiver | CsSetupTarget::SideEffect => None,
    };
    params
        .iter()
        .map(|(name, ty)| {
            let value = role.and_then(|r| inputs.get(&format!("{name}_{r}"))).or_else(|| inputs.get(name));
            yaml_to_csharp_literal(value, ty)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_csharp_op_args(
    out: &mut String,
    cs_op: &CsOp,
    inputs: &BTreeMap<String, serde_yaml::Value>,
    bindings: &[CsSetupBinding],
    op_defaults: Option<&BTreeMap<String, serde_yaml::Value>>,
    step_idx: usize,
) -> String {
    use std::fmt::Write as _;
    let mut args = Vec::new();
    for (param_name, param_type) in &cs_op.params {
        if let Some(binding) = bindings
            .iter()
            .find(|b| matches!(&b.target, CsSetupTarget::Param(name) if name == param_name))
        {
            if cs_op.return_type.trim() != "void" {
                writeln!(
                    out,
                    "    SpecGateRuntime.EmitEvent(\"{}.{param_name}\", {});",
                    cs_op.op_name, binding.var
                )
                .expect("fmt");
            }
            args.push(binding.var.clone());
            continue;
        }
        let value = inputs.get(param_name).or_else(|| op_defaults.and_then(|d| d.get(param_name)));
        let lit = yaml_to_csharp_literal(value, param_type);
        let var = format!("__sg_arg_{step_idx}_{}", csharp_ident(param_name));
        writeln!(out, "    {param_type} {var} = {lit};").expect("fmt");
        if is_cs_option_type(param_type) || is_nullable_cs_type(param_type) {
            let echo = yaml_to_csharp_echo_literal(value);
            writeln!(out, "    SpecGateRuntime.EmitEvent(\"{}.{param_name}\", {echo});", cs_op.op_name).expect("fmt");
        } else {
            let emit = cs_typed_emit(&format!("{}.{param_name}", cs_op.op_name), &var, param_type);
            writeln!(out, "    {emit}").expect("fmt");
        }
        args.push(var);
    }
    args.join(", ")
}

fn render_csharp_op_call(cs_op: &CsOp, bindings: &[CsSetupBinding], args: &str) -> String {
    if cs_op.is_static {
        return format!("{}.{}({args})", cs_op.class_name, cs_op.method_name);
    }
    let receiver = bindings
        .iter()
        .find(|b| b.target == CsSetupTarget::Receiver)
        .map_or_else(|| "/* missing receiver */".to_string(), |b| b.var.clone());
    format!("{receiver}.{}({args})", cs_op.method_name)
}

fn csharp_ident(name: &str) -> String {
    let mut out = String::new();
    for ch in name.trim_start_matches('@').chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() { "arg".to_string() } else { out }
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

#[derive(Default)]
pub(crate) struct CsProjectSettings {
    framework: Option<String>,
    nullable: Option<String>,
    implicit_usings: Option<String>,
    lang_version: Option<String>,
}

/// Find the first `.csproj` in `package_root` and return compilation settings
/// that affect whether copied fixture source compiles in the scratch runner.
pub(crate) fn read_csproj_settings(package_root: &Path) -> CsProjectSettings {
    let Ok(entries) = std::fs::read_dir(package_root) else {
        return CsProjectSettings::default();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("csproj") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                return CsProjectSettings::default();
            };
            return CsProjectSettings {
                framework: extract_csproj_xml_tag(&text, "TargetFramework").or_else(|| {
                    extract_csproj_xml_tag(&text, "TargetFrameworks").and_then(|fws| fws.split(';').next().map(|s| s.trim().to_string()))
                }),
                nullable: extract_csproj_xml_tag(&text, "Nullable"),
                implicit_usings: extract_csproj_xml_tag(&text, "ImplicitUsings"),
                lang_version: extract_csproj_xml_tag(&text, "LangVersion"),
            };
        }
    }
    CsProjectSettings::default()
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
#[cfg(test)]
pub(crate) fn resolve_framework_for_runner(target: &binding::Target) -> String {
    resolve_csharp_runner_settings(target).framework
}

pub(crate) struct CSharpRunnerSettings {
    framework: String,
    nullable: String,
    implicit_usings: String,
    lang_version: Option<String>,
}

pub(crate) fn resolve_csharp_runner_settings(target: &binding::Target) -> CSharpRunnerSettings {
    const DEFAULT: &str = "net10.0";
    let project = read_csproj_settings(&target.package_root);
    let fw = target
        .framework
        .clone()
        .or(project.framework)
        .unwrap_or_else(|| DEFAULT.to_string());
    let framework = if fw.starts_with("netstandard") { DEFAULT.to_string() } else { fw };
    CSharpRunnerSettings {
        framework,
        nullable: project.nullable.unwrap_or_else(|| "enable".to_string()),
        implicit_usings: project.implicit_usings.unwrap_or_else(|| "enable".to_string()),
        lang_version: project.lang_version,
    }
}

fn prepare_csharp_sources(package_root: &Path, scratch_dir: &Path, source_files: &BTreeSet<PathBuf>) -> Result<PathBuf, String> {
    let source_dir = scratch_dir.join("source");
    std::fs::create_dir_all(&source_dir).map_err(|e| format!("failed to create C# source dir: {e}"))?;
    copy_selected_csharp_sources(package_root, &source_dir, source_files)?;
    Ok(source_dir)
}

fn copy_selected_csharp_sources(root: &Path, out_root: &Path, source_files: &BTreeSet<PathBuf>) -> Result<(), String> {
    for path in source_files {
        let rel = path
            .strip_prefix(root)
            .map_err(|e| format!("failed to relativize C# source path: {e}"))?;
        if is_skipped_csharp_rel_path(rel) {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("cs") {
            let text = std::fs::read_to_string(path).map_err(|e| format!("failed to read C# source '{}': {e}", path.display()))?;
            let out_path = out_root.join(rel);
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("failed to create C# source output dir: {e}"))?;
            }
            let instrumented =
                instrument_csharp_source(&text).map_err(|e| format!("failed to instrument C# source '{}': {e}", path.display()))?;
            std::fs::write(out_path, instrumented).map_err(|e| format!("failed to write instrumented C# source: {e}"))?;
        }
    }
    Ok(())
}

fn is_skipped_csharp_rel_path(path: &Path) -> bool {
    path.components().any(|component| {
        let part = component.as_os_str().to_string_lossy();
        matches!(part.as_ref(), "Tests" | "bin" | "obj")
    })
}

fn emit_csharp_mock_tables(out: &mut String, inputs: &BTreeMap<String, serde_yaml::Value>) {
    use std::fmt::Write as _;

    for (name, value) in inputs {
        let serde_yaml::Value::Mapping(entries) = value else {
            continue;
        };
        writeln!(
            out,
            "    SpecGateRuntime.SetMock({}, new Dictionary<string, string>",
            csharp_string_literal(name)
        )
        .expect("fmt");
        out.push_str("    {\n");
        for (key, response) in entries {
            if let (Some(key), Some(response)) = (key.as_str(), response.as_str()) {
                writeln!(
                    out,
                    "        [{}] = {},",
                    csharp_string_literal(key),
                    csharp_string_literal(response)
                )
                .expect("fmt");
            }
        }
        out.push_str("    });\n");
    }
}

fn instrument_csharp_source(text: &str) -> Result<String, String> {
    use std::fmt::Write as _;

    let mut out = String::new();
    let mut pending_spec_event_attrs: Vec<String> = Vec::new();
    let mut pending_spec_mock: Option<String> = None;
    let mut mock_fields: BTreeMap<String, String> = BTreeMap::new();
    let mut pending_operation_attrs: Vec<String> = Vec::new();
    let mut pending_operation_sig = String::new();
    let mut operation_return_type: Option<String> = None;
    let mut pending_operation_instrumentation: Option<CsOperationInstrumentation> = None;
    let mut operation_brace_depth = 0usize;
    let mut mock_counter = 0usize;

    for line in text.lines() {
        let trimmed = line.trim();
        let in_operation = operation_return_type.is_some() && operation_brace_depth > 0;
        if in_operation {
            if let Some(rewritten) = rewrite_csharp_mock_line(
                line,
                operation_return_type.as_deref().unwrap_or("void"),
                &mock_fields,
                &mut mock_counter,
            )? {
                out.push_str(&rewritten);
                out.push('\n');
            } else if line_contains_mock_receiver_call(line, &mock_fields) {
                return Err(format!("unsupported [SpecMock] call shape in operation body: {trimmed}"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
            update_csharp_brace_depth(line, &mut operation_brace_depth);
            if operation_brace_depth == 0 {
                operation_return_type = None;
            }
            continue;
        }

        if let Some(class_name) = extract_cs_class(trimmed) {
            mock_fields.clear();
            pending_spec_mock = None;
            let _ = class_name;
        }

        if let Some(mock_name) = extract_spec_mock_attr(trimmed) {
            pending_spec_mock = Some(mock_name);
            out.push_str(line);
            out.push('\n');
            continue;
        }

        if let Some(mock_name) = pending_spec_mock.take()
            && let Some(field_name) = parse_csharp_field_name(line)
        {
            mock_fields.insert(field_name, mock_name);
        }

        if let Some((op_name, _component)) = extract_spec_operation_attr(trimmed) {
            if !pending_operation_attrs.iter().any(|existing| existing == &op_name) {
                pending_operation_attrs.push(op_name);
            }
            pending_operation_sig.clear();
        }

        if !pending_operation_attrs.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('[') {
            if !pending_operation_sig.is_empty() {
                pending_operation_sig.push(' ');
            }
            pending_operation_sig.push_str(trimmed);
            if let Some((_method_name, params, return_type, _is_static)) = extract_cs_method_sig(&pending_operation_sig) {
                let instrumentation = CsOperationInstrumentation {
                    operation: pending_operation_attrs[0].clone(),
                    params: parse_cs_param_infos_from_sig(&pending_operation_sig).unwrap_or_else(|| {
                        params
                            .iter()
                            .map(|(name, _ty)| CsParamInfo {
                                spec_name: name.clone(),
                                code_name: csharp_ident(name),
                            })
                            .collect()
                    }),
                    return_type: return_type.clone(),
                };
                pending_operation_attrs.clear();
                pending_operation_sig.clear();

                if trimmed.contains("=>") {
                    if let Some(rewritten) = rewrite_csharp_expression_bodied_operation(line, &instrumentation) {
                        out.push_str(&rewritten);
                        operation_return_type = None;
                        pending_operation_instrumentation = None;
                        continue;
                    }
                    operation_return_type = None;
                    pending_operation_instrumentation = None;
                } else {
                    operation_return_type = Some(return_type);
                    pending_operation_instrumentation = Some(instrumentation);
                }
            }
        }

        if trimmed.starts_with("[SpecEvent") {
            pending_spec_event_attrs.push(line.to_string());
            continue;
        }
        if !pending_spec_event_attrs.is_empty()
            && let Some(prop) = parse_csharp_auto_property(line)
        {
            let event_name = pending_spec_event_attrs
                .iter()
                .find_map(|attr| extract_spec_event_attr_name(attr))
                .unwrap_or_else(|| prop.name.trim_start_matches('@').to_string());
            let field_name = format!("__sg_{}", csharp_ident(&prop.name));
            writeln!(out, "{}private {} {};", prop.indent, prop.ty, field_name).expect("fmt");
            for attr in pending_spec_event_attrs.drain(..) {
                out.push_str(&attr);
                out.push('\n');
            }
            writeln!(out, "{}{} {} {}", prop.indent, prop.visibility, prop.ty, prop.name).expect("fmt");
            writeln!(out, "{}{{", prop.indent).expect("fmt");
            writeln!(out, "{}    get => {};", prop.indent, field_name).expect("fmt");
            writeln!(out, "{}    set", prop.indent).expect("fmt");
            writeln!(out, "{}    {{", prop.indent).expect("fmt");
            writeln!(out, "{}        {} = value;", prop.indent, field_name).expect("fmt");
            writeln!(
                out,
                "{}        SpecGate.Runtime.SpecGateRuntime.EmitMember(this, {}, value);",
                prop.indent,
                csharp_string_literal(&event_name)
            )
            .expect("fmt");
            writeln!(out, "{}    }}", prop.indent).expect("fmt");
            writeln!(out, "{}}}", prop.indent).expect("fmt");
            continue;
        }
        for attr in pending_spec_event_attrs.drain(..) {
            out.push_str(&attr);
            out.push('\n');
        }
        out.push_str(line);
        out.push('\n');
        if operation_return_type.is_some() && operation_brace_depth == 0 && line.contains('{') {
            update_csharp_brace_depth(line, &mut operation_brace_depth);
            if operation_brace_depth > 0
                && let Some(instrumentation) = pending_operation_instrumentation.take()
            {
                let indent = format!("{}    ", &line[..line.len() - line.trim_start().len()]);
                out.push_str(&render_csharp_operation_entry(&instrumentation, &indent));
            }
        }
    }
    for attr in pending_spec_event_attrs {
        out.push_str(&attr);
        out.push('\n');
    }
    Ok(out)
}

struct CsOperationInstrumentation {
    operation: String,
    params: Vec<CsParamInfo>,
    return_type: String,
}

struct CsParamInfo {
    spec_name: String,
    code_name: String,
}

fn parse_cs_param_infos_from_sig(sig: &str) -> Option<Vec<CsParamInfo>> {
    let paren_open = sig.find('(')?;
    let paren_close = find_matching_paren(sig, paren_open)?;
    let params_str = sig[paren_open + 1..paren_close].trim();
    if params_str.is_empty() {
        return Some(Vec::new());
    }
    Some(split_cs_params(params_str).into_iter().filter_map(parse_cs_param_info).collect())
}

fn parse_cs_param_info(param: &str) -> Option<CsParamInfo> {
    let (spec_name, without_attrs) = peel_spec_input_attrs(param.trim());
    let member = parse_csharp_member_prefix(without_attrs)?;
    let code_name = member.name;
    let event_name = spec_name.unwrap_or_else(|| code_name.trim_start_matches('@').to_string());
    Some(CsParamInfo {
        spec_name: event_name,
        code_name,
    })
}

fn render_csharp_operation_entry(inst: &CsOperationInstrumentation, indent: &str) -> String {
    let names = if inst.params.is_empty() {
        "System.Array.Empty<string>()".to_string()
    } else {
        format!(
            "new string[] {{ {} }}",
            inst.params
                .iter()
                .map(|p| csharp_string_literal(&p.spec_name))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let values = if inst.params.is_empty() {
        "System.Array.Empty<object?>()".to_string()
    } else {
        format!(
            "new object?[] {{ {} }}",
            inst.params.iter().map(|p| p.code_name.clone()).collect::<Vec<_>>().join(", ")
        )
    };
    format!(
        "{indent}SpecGate.Runtime.SpecGateRuntime.EnterOperation({}, {names}, {values});\n",
        csharp_string_literal(&inst.operation)
    )
}

fn rewrite_csharp_expression_bodied_operation(line: &str, inst: &CsOperationInstrumentation) -> Option<String> {
    let arrow = line.find("=>")?;
    let semi = line.rfind(';')?;
    if semi < arrow {
        return None;
    }
    let prefix = line[..arrow].trim_end();
    let expr = line[arrow + 2..semi].trim();
    let indent = &line[..line.len() - line.trim_start().len()];
    let body_indent = format!("{indent}    ");
    let mut out = String::new();
    out.push_str(prefix);
    out.push('\n');
    out.push_str(indent);
    out.push_str("{\n");
    out.push_str(&render_csharp_operation_entry(inst, &body_indent));
    if inst.return_type.trim() == "void" {
        out.push_str(&body_indent);
        out.push_str(expr);
        out.push_str(";\n");
    } else {
        out.push_str(&body_indent);
        out.push_str("return ");
        out.push_str(expr);
        out.push_str(";\n");
    }
    out.push_str(indent);
    out.push_str("}\n");
    Some(out)
}

fn parse_csharp_field_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let before_init = trimmed
        .split_once('=')
        .map_or(trimmed.trim_end_matches(';'), |(before, _)| before.trim());
    let member = parse_csharp_member_prefix(before_init)?;
    if member.modifiers.iter().any(|m| m == "static") {
        return None;
    }
    Some(member.name)
}

fn extract_spec_mock_attr(line: &str) -> Option<String> {
    let s = line.trim();
    let rest = s.strip_prefix("[SpecMock(\"")?;
    let name = rest.split('"').next()?;
    Some(name.to_string())
}

fn rewrite_csharp_mock_line(
    line: &str,
    operation_return_type: &str,
    mock_fields: &BTreeMap<String, String>,
    counter: &mut usize,
) -> Result<Option<String>, String> {
    let indent = &line[..line.len() - line.trim_start().len()];
    let trimmed = line.trim();
    if !trimmed.ends_with(';') {
        return Ok(None);
    }

    if let Some(rest) = trimmed.strip_prefix("return ") {
        let expr = rest.trim_end_matches(';').trim();
        if let Some(call) = parse_csharp_mock_call(expr, mock_fields)? {
            let hit = next_csharp_mock_hit(counter);
            return Ok(Some(format!(
                "{indent}var {hit}_value = SpecGate.Runtime.SpecGateRuntime.MockCall<{}>({}, {}, out var {hit}); if (!{hit}) return SpecGate.Runtime.SpecGateRuntime.SpecDefault<{}>(); return {hit}_value;",
                operation_return_type.trim(),
                csharp_string_literal(&call.mock_name),
                call.key_expr,
                operation_return_type.trim()
            )));
        }
        return Ok(None);
    }

    let Some((lhs, rhs_with_semicolon)) = trimmed.split_once('=') else {
        return Ok(None);
    };
    let rhs = rhs_with_semicolon.trim_end_matches(';').trim();
    let Some(call) = parse_csharp_mock_call(rhs, mock_fields)? else {
        return Ok(None);
    };
    let hit = next_csharp_mock_hit(counter);
    if let Some(member) = parse_csharp_member_prefix(lhs.trim()) {
        let call_type = if member.ty == "var" { "string" } else { member.ty.as_str() };
        return Ok(Some(format!(
            "{indent}{} = SpecGate.Runtime.SpecGateRuntime.MockCall<{}>({}, {}, out var {hit}); if (!{hit}) return SpecGate.Runtime.SpecGateRuntime.SpecDefault<{}>();",
            lhs.trim(),
            call_type.trim(),
            csharp_string_literal(&call.mock_name),
            call.key_expr,
            operation_return_type.trim()
        )));
    }

    Ok(Some(format!(
        "{indent}{} = SpecGate.Runtime.SpecGateRuntime.MockCall<string>({}, {}, out var {hit}); if (!{hit}) return SpecGate.Runtime.SpecGateRuntime.SpecDefault<{}>();",
        lhs.trim(),
        csharp_string_literal(&call.mock_name),
        call.key_expr,
        operation_return_type.trim()
    )))
}

fn next_csharp_mock_hit(counter: &mut usize) -> String {
    let value = format!("__sg_mock_hit_{counter}");
    *counter += 1;
    value
}

struct CsMockCall {
    mock_name: String,
    key_expr: String,
}

fn parse_csharp_mock_call(expr: &str, mock_fields: &BTreeMap<String, String>) -> Result<Option<CsMockCall>, String> {
    let expr = expr.trim();
    for (field, mock_name) in mock_fields {
        for prefix in [format!("{field}."), format!("this.{field}.")] {
            if let Some(rest) = expr.strip_prefix(&prefix) {
                let Some(paren_open) = rest.find('(') else {
                    continue;
                };
                let method = rest[..paren_open].trim();
                if method.is_empty() || !method.chars().all(is_csharp_ident_continue) {
                    continue;
                }
                let paren_close =
                    find_matching_paren(rest, paren_open).ok_or_else(|| format!("invalid [SpecMock] call expression: {expr}"))?;
                if rest[paren_close + 1..].trim().is_empty() {
                    let args = rest[paren_open + 1..paren_close].trim();
                    let split = split_cs_params(args);
                    let Some(last_arg) = split.last() else {
                        return Err(format!("[SpecMock] call must have at least one argument: {expr}"));
                    };
                    let last_arg = last_arg.trim();
                    if last_arg.starts_with("ref ") || last_arg.starts_with("out ") {
                        return Err(format!("[SpecMock] call cannot use ref/out arguments: {expr}"));
                    }
                    return Ok(Some(CsMockCall {
                        mock_name: mock_name.clone(),
                        key_expr: last_arg.to_string(),
                    }));
                }
            }
        }
    }
    Ok(None)
}

fn line_contains_mock_receiver_call(line: &str, mock_fields: &BTreeMap<String, String>) -> bool {
    mock_fields
        .keys()
        .any(|field| line.contains(&format!("{field}.")) || line.contains(&format!("this.{field}.")))
}

fn update_csharp_brace_depth(line: &str, depth: &mut usize) {
    for ch in line.chars() {
        match ch {
            '{' => *depth += 1,
            '}' => *depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

struct CsAutoProperty {
    indent: String,
    visibility: String,
    ty: String,
    name: String,
}

fn parse_csharp_auto_property(line: &str) -> Option<CsAutoProperty> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = line[..indent_len].to_string();
    let trimmed = line.trim();
    let before_brace = trimmed.split('{').next()?.trim();
    let body = trimmed.split('{').nth(1)?.trim_end_matches('}').trim();
    if !body.contains("get;") || !body.contains("set;") {
        return None;
    }
    let member = parse_csharp_member_prefix(before_brace)?;
    if member.modifiers.iter().any(|m| m == "static") {
        return None;
    }
    Some(CsAutoProperty {
        indent,
        visibility: member.modifiers.join(" "),
        ty: member.ty,
        name: member.name,
    })
}

fn extract_spec_event_attr_name(attr: &str) -> Option<String> {
    let open = attr.find("SpecEvent(\"")?;
    let rest = &attr[open + "SpecEvent(\"".len()..];
    Some(rest.split('"').next()?.to_string())
}

fn csharp_fixture_event_names(package_root: &Path) -> BTreeSet<String> {
    let mut files = Vec::new();
    collect_csharp_files(package_root, &mut files);
    let mut names = BTreeSet::new();
    for file in files {
        if let Ok(text) = std::fs::read_to_string(file) {
            collect_csharp_event_names(&text, &mut names);
        }
    }
    names
}

fn collect_csharp_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_csharp_files(&path, files);
        } else if path.extension().and_then(|e| e.to_str()) == Some("cs") {
            files.push(path);
        }
    }
}

fn collect_csharp_event_names(text: &str, names: &mut BTreeSet<String>) {
    let mut pending: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[SpecEvent") {
            pending.push(line.to_string());
            continue;
        }
        if pending.is_empty() {
            continue;
        }
        let attr_name = pending.iter().find_map(|attr| extract_spec_event_attr_name(attr));
        if let Some(prop) = parse_csharp_auto_property(line) {
            names.insert(attr_name.unwrap_or_else(|| prop.name.trim_start_matches('@').to_string()));
        }
        pending.clear();
    }
}

/// Run one C# target group: scan annotated operations, generate and execute a
/// temporary C# runner, parse its trace output, and return per-case results.
fn run_csharp_group(
    target: &binding::Target,
    group_cases: &[&spec::Case],
    spec: &spec::Spec,
    scratch_dir: &Path,
) -> Result<Vec<CaseResult>, String> {
    let (all_cs_ops, all_cs_setups) = scan_csharp(&target.package_root);
    let component = &spec.name;

    // Verify all required operations have C# annotations in the correct component.
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

    // Filter to operations matching this component, error loudly if not found.
    let cs_ops: Vec<CsOp> = all_cs_ops
        .into_iter()
        .filter(|op| op.component.as_deref() == Some(component))
        .collect();

    let cs_setups: Vec<CsSetup> = all_cs_setups
        .into_iter()
        .filter(|setup| setup.component.as_deref() == Some(component) || setup.component.is_none())
        .collect();

    for op in &required_ops {
        let matching = cs_ops.iter().filter(|co| co.op_name == *op).collect::<Vec<_>>();
        match matching.len() {
            0 => return Err(format!("operation '{op}' not found in C# for component '{component}'")),
            1 => {}
            n => return Err(format!("operation '{op}' defined {n} times in component '{component}'")),
        }
    }

    let mut source_files = BTreeSet::new();
    for op in &required_ops {
        if let Some(cs_op) = cs_ops.iter().find(|co| co.op_name == *op) {
            source_files.insert(cs_op.source_path.clone());
        }
    }
    for case in group_cases {
        let ops = csharp_case_ops(case);
        let bindings = resolve_csharp_case(&cs_ops, &cs_setups, &ops)
            .map_err(|detail| format!("C# case '{}' setup wiring failed: {detail}", case.name))?;
        for binding in bindings {
            source_files.insert(binding.setup.source_path);
        }
    }

    std::fs::create_dir_all(scratch_dir).map_err(|e| format!("failed to create C# scratch dir: {e}"))?;
    let trace_file = scratch_dir.join("traces.json");

    // Resolve project settings for the runner (binding framework > csproj >
    // defaults, with netstandard falling back to net10.0 since it can't produce
    // an exe). These mirror source-affecting fixture project settings so
    // dotnet-format-canonical source still compiles in the scratch project.
    let runner_settings = resolve_csharp_runner_settings(target);
    let lang_version = runner_settings
        .lang_version
        .as_ref()
        .map(|v| format!("    <LangVersion>{}</LangVersion>\n", escape_xml_text(v)))
        .unwrap_or_default();

    // Write Runner.csproj that compiles the fixture source directly. This keeps
    // concurrent harness runs from contending on the fixture project's obj/bin.
    let source_dir = prepare_csharp_sources(&target.package_root, scratch_dir, &source_files)?;
    let pkg_path = path_to_forward_slash(&source_dir);
    let csharp_libs_dir = find_csharp_libs_dir(&target.package_root);
    let runtime_sources = match &csharp_libs_dir {
        Some(libs) => {
            let mut lines: Vec<(String, String)> = Vec::new();
            for sub in ["SpecGate.Annotations", "SpecGate.Runtime"] {
                let dir = libs.join(sub);
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("cs") {
                            let fwd = path_to_forward_slash(&path);
                            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                            lines.push((fwd, name));
                        }
                    }
                }
            }
            lines.sort_by(|a, b| a.1.cmp(&b.1));
            let items = lines
                .iter()
                .map(|(fwd, name)| format!("    <Compile Include=\"{fwd}\" Link=\"{name}\" />"))
                .collect::<Vec<_>>()
                .join("\n");
            format!("  <ItemGroup>\n{items}\n  </ItemGroup>\n")
        }
        None => String::new(),
    };
    let csproj = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n  <PropertyGroup>\n    \
         <OutputType>Exe</OutputType>\n    <TargetFramework>{}</TargetFramework>\n    \
         <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n    \
         <Nullable>{}</Nullable>\n    <ImplicitUsings>{}</ImplicitUsings>\n{}  </PropertyGroup>\n  <ItemGroup>\n    \
         <Compile Include=\"Program.cs\" />\n    \
         <Compile Include=\"{pkg_path}/**/*.cs\" Exclude=\"{pkg_path}/Tests/**/*.cs;{pkg_path}/bin/**/*.cs;{pkg_path}/obj/**/*.cs\" LinkBase=\"Fixture\" />\n  \
         </ItemGroup>\n{runtime_sources}</Project>\n",
        escape_xml_text(&runner_settings.framework),
        escape_xml_text(&runner_settings.nullable),
        escape_xml_text(&runner_settings.implicit_usings),
        lang_version
    );
    std::fs::write(scratch_dir.join("Runner.csproj"), csproj).map_err(|e| format!("failed to write Runner.csproj: {e}"))?;

    // Write Program.cs.
    let program_cs = generate_csharp_program(group_cases, &cs_ops, &cs_setups, &spec.op_input_defaults)?;
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

#[derive(Debug, Deserialize)]
struct DiscoveryRegistry {
    operations: Vec<DiscoveryOp>,
}

#[derive(Debug, Deserialize)]
struct DiscoveryOp {
    name: String,
    module_path: String,
    component: String,
    #[serde(default)]
    is_setup: bool,
}

fn run_discovery(package_root: &Path) -> Result<DiscoveryRegistry, String> {
    let ws = workspace_root();
    let specgate_path = ws.join("crates").join("specgate");
    let pkg_abs = std::fs::canonicalize(package_root).map_err(|e| format!("cannot resolve package_root: {e}"))?;
    let crate_name = cargo_package_name(package_root).ok_or_else(|| "could not read crate name from Cargo.toml".to_string())?;
    let rust_ident = crate_name.replace('-', "_");

    let scratch = ws.join("target").join("specgate-harness-discovery").join(&crate_name);
    std::fs::create_dir_all(scratch.join("src")).map_err(|e| format!("failed to scaffold discovery crate: {e}"))?;

    let manifest = format!(
        "[package]\nname = \"sg-harness-discovery\"\nversion = \"0.0.1\"\nedition = \"2024\"\n\n[[bin]]\nname = \"sg-harness-discovery\"\npath = \"src/main.rs\"\n\n[dependencies]\nspecgate = {{ path = \"{}\" }}\n{} = {{ path = \"{}\" }}\n\n[workspace]\n",
        cargo_path(&specgate_path),
        crate_name,
        cargo_path(&pkg_abs),
    );
    std::fs::write(scratch.join("Cargo.toml"), manifest).map_err(|e| format!("failed to write discovery manifest: {e}"))?;

    let parent_lock = ws.join("Cargo.lock");
    if parent_lock.exists() {
        let _ = std::fs::copy(&parent_lock, scratch.join("Cargo.lock"));
    }

    let main_rs = format!("extern crate {rust_ident};\nfn main() {{\n    print!(\"{{}}\", specgate::__rt::discovery_json());\n}}\n");
    std::fs::write(scratch.join("src").join("main.rs"), main_rs).map_err(|e| format!("failed to write discovery main.rs: {e}"))?;

    let mut cmd = Command::new(cargo_bin());
    cmd.arg("run").arg("--quiet").arg("--manifest-path").arg(scratch.join("Cargo.toml"));
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
    cmd.env("CARGO_TARGET_DIR", scratch.join("target").as_os_str());

    let output = cmd.output().map_err(|e| format!("failed to run discovery build: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "discovery build failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let json = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if json.is_empty() {
        return Err("discovery build produced no output".to_string());
    }
    serde_json::from_str(&json).map_err(|e| format!("discovery build produced invalid JSON: {e}"))
}

fn cargo_package_name(package_root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(package_root.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[package]" {
            in_package = true;
            continue;
        }
        if trimmed.starts_with('[') {
            in_package = false;
        }
        if in_package && let Some(rest) = trimmed.strip_prefix("name") {
            let rest = rest.trim_start_matches([' ', '\t', '=']).trim();
            let name = rest.trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn cargo_path(p: &Path) -> String {
    let s = p.display().to_string();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.replace('\\', "/")
}

fn required_operations(cases: &[&spec::Case]) -> BTreeSet<String> {
    let mut req_ops = BTreeSet::new();
    for case in cases {
        if !case.steps.is_empty() {
            req_ops.extend(case.steps.iter().cloned());
        } else if let Some(op) = case.operation.as_deref() {
            req_ops.insert(op.to_string());
        }
    }
    req_ops
}

fn metadata_fixture_sources(
    package_root: &Path,
    component: &str,
    cases: &[&spec::Case],
    registry: &DiscoveryRegistry,
) -> Result<Option<Vec<PathBuf>>, String> {
    let req_ops = required_operations(cases);
    if req_ops.is_empty() {
        return Ok(None);
    }
    let crate_ident = cargo_package_name(package_root).map(|n| n.replace('-', "_"));
    let mut sources = Vec::new();
    for op in req_ops {
        let matches: Vec<&DiscoveryOp> = registry
            .operations
            .iter()
            .filter(|m| !m.is_setup && m.component == component && m.name == op)
            .collect();
        match matches.as_slice() {
            [] => return Ok(None),
            [meta] => {
                let module_path = relative_module_path(&meta.module_path, crate_ident.as_deref());
                let source = codegen::source_for_module_path(package_root, &module_path)
                    .ok_or_else(|| format!("source file not found for operation '{op}' in module '{}'", meta.module_path))?;
                if !sources.contains(&source) {
                    sources.push(source);
                }
            }
            _ => return Err(format!("operation '{op}' is defined more than once in component '{component}'")),
        }
    }
    Ok(Some(sources))
}

fn relative_module_path(module_path: &str, crate_ident: Option<&str>) -> Vec<String> {
    let mut parts: Vec<String> = module_path
        .split("::")
        .filter(|s| !s.is_empty())
        .map(|s| s.strip_prefix("r#").unwrap_or(s).to_string())
        .collect();
    if crate_ident.is_some_and(|ident| parts.first().is_some_and(|first| first == ident)) {
        parts.remove(0);
    }
    parts
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
/// physical file, read it directly. Otherwise, for a directory module, merge
/// all `.rs` files in that directory.
fn load_fixture_text(fixture_src: &Path) -> Result<String, String> {
    if fixture_src.exists() {
        if fixture_src.is_dir() {
            return merge_module_dir(fixture_src).ok_or_else(|| format!("module directory empty: {}", fixture_src.display()));
        }
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

fn check_shape(
    spec: &spec::Spec,
    raw: &serde_yaml::Value,
    bindings: &[(String, binding::Binding)],
    _fixture_basename: &str,
) -> Option<String> {
    let ops_meta = ops_metadata(raw);
    let mut setup_event_names = BTreeSet::new();
    for (_, binding) in bindings {
        if binding.language == "csharp"
            && let Some(target) = binding.target(spec.target.as_deref())
        {
            setup_event_names.extend(csharp_fixture_event_names(&target.package_root));
        }
    }
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
                if setup_event_names.contains(k) {
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

fn reported_expected_for_case(case: &spec::Case, raw: &serde_yaml::Value) -> Vec<Assertion> {
    let ops_meta = ops_metadata(raw);
    let allowed = allowed_report_events(case, &ops_meta);
    case.expected
        .iter()
        .filter(|entry| {
            if matches!(entry, Assertion::Run { .. }) {
                return true;
            }
            let mut leaf_names = Vec::new();
            collect_event_names(entry, &mut leaf_names);
            leaf_names.is_empty()
                || leaf_names
                    .iter()
                    .any(|name| allowed.contains(name) || allowed.iter().any(|op| name.starts_with(&format!("{op}."))))
        })
        .cloned()
        .collect()
}

fn allowed_report_events(case: &spec::Case, ops_meta: &BTreeMap<String, OpMeta>) -> BTreeSet<String> {
    let mut allowed = BTreeSet::new();
    let case_ops: Vec<&str> = if case.steps.is_empty() {
        case.operation.as_deref().into_iter().collect()
    } else {
        case.steps.iter().map(String::as_str).collect()
    };
    for op in &case_ops {
        if let Some(meta) = ops_meta.get(*op) {
            for inp in &meta.inputs {
                allowed.insert(format!("{op}.{inp}"));
            }
            for out in &meta.outputs {
                allowed.insert(out.clone());
            }
        }
        allowed.insert((*op).to_string());
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
    allowed
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
    use super::{
        CsOp, DiscoveryOp, DiscoveryRegistry, extract_cs_method_sig, generate_csharp_program, metadata_fixture_sources, ops_metadata,
        parse_csharp_auto_property, scan_csharp_file, split_cs_params, yaml_to_csharp_literal,
    };
    use crate::binding;
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    fn write_file(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, text).unwrap();
    }

    fn case_with_op(op: &str) -> super::spec::Case {
        super::spec::Case {
            name: op.to_string(),
            target: None,
            operation: Some(op.to_string()),
            steps: Vec::new(),
            step_inputs: Vec::new(),
            inputs: BTreeMap::new(),
            expected: Vec::new(),
            level: super::CaseLevel::Must,
            source: None,
        }
    }

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
    fn csharp_scanner_records_spec_exception_and_nullable_returns() {
        let src = r#"
            namespace Demo;
            public static class Ops {
                [SpecOperation("find")]
                public static Point? Find() => null;

                [SpecOperation("checked_divide")]
                [SpecException(typeof(DivideByZeroException))]
                public static int CheckedDivide(int a, int b) => a / b;

                [SpecOperation("parse")]
                [SpecException]
                public static int Parse(string input) => int.Parse(input);
            }
            public class Point {}
        "#;
        let mut ops = Vec::new();
        let mut setups = Vec::new();
        scan_csharp_file(src, Path::new("fixture.cs"), &mut ops, &mut setups);

        let find = ops.iter().find(|op| op.op_name == "find").expect("find op");
        assert!(find.return_nullable);
        assert!(find.exception_types.is_none());
        assert!(find.is_static);

        let checked = ops.iter().find(|op| op.op_name == "checked_divide").expect("checked op");
        assert_eq!(
            checked.exception_types.as_ref().expect("exception types"),
            &vec!["DivideByZeroException".to_string()]
        );
        assert!(checked.is_static, "SpecException must not disrupt static detection");

        let catch_all = ops.iter().find(|op| op.op_name == "parse").expect("parse op");
        assert_eq!(catch_all.exception_types.as_ref().expect("catch-all types"), &Vec::<String>::new());
    }

    #[test]
    fn csharp_program_wraps_nullable_and_exception_results() {
        let option_case = case_with_op("find");
        let result_case = case_with_op("checked_divide");
        let catch_all_case = case_with_op("parse");
        let ops = vec![
            CsOp {
                op_name: "find".to_string(),
                component: None,
                source_path: PathBuf::new(),
                class_name: "Ops".to_string(),
                method_of: "Ops".to_string(),
                method_name: "Find".to_string(),
                params: Vec::new(),
                return_type: "Point?".to_string(),
                return_nullable: true,
                exception_types: None,
                is_static: true,
            },
            CsOp {
                op_name: "checked_divide".to_string(),
                component: None,
                source_path: PathBuf::new(),
                class_name: "Ops".to_string(),
                method_of: "Ops".to_string(),
                method_name: "CheckedDivide".to_string(),
                params: Vec::new(),
                return_type: "int".to_string(),
                return_nullable: false,
                exception_types: Some(vec!["DivideByZeroException".to_string()]),
                is_static: true,
            },
            CsOp {
                op_name: "parse".to_string(),
                component: None,
                source_path: PathBuf::new(),
                class_name: "Ops".to_string(),
                method_of: "Ops".to_string(),
                method_name: "Parse".to_string(),
                params: Vec::new(),
                return_type: "int".to_string(),
                return_nullable: false,
                exception_types: Some(Vec::new()),
                is_static: true,
            },
        ];

        let program =
            generate_csharp_program(&[&option_case, &result_case, &catch_all_case], &ops, &[], &BTreeMap::new()).expect("program");

        assert!(program.contains("SpecGateRuntime.EmitOptionResult(__sg_result_0);"));
        assert!(program.contains("SpecGateRuntime.EmitTaggedResult(\"Ok\", __sg_result_0);"));
        assert!(program.contains("catch (DivideByZeroException __ex)"));
        assert!(program.contains("SpecGateRuntime.EmitTaggedResult(\"Err\", __ex.Message);"));
        assert!(program.contains("catch (System.Exception __ex)"));
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
            component: Some("fixture.default_input".to_string()),
            source_path: PathBuf::new(),
            class_name: "ScaleOps".to_string(),
            method_of: "ScaleOps".to_string(),
            method_name: "Scale".to_string(),
            params: vec![("value".to_string(), "int".to_string()), ("factor".to_string(), "int".to_string())],
            return_type: "int".to_string(),
            return_nullable: false,
            exception_types: None,
            is_static: true,
        };
        let mut scale_defaults = BTreeMap::new();
        scale_defaults.insert("factor".to_string(), serde_yaml::Value::Number(2.into()));
        let mut defaults = BTreeMap::new();
        defaults.insert("scale".to_string(), scale_defaults);

        let program = generate_csharp_program(&[&case], &[op], &[], &defaults).expect("program");

        assert!(program.contains("int __sg_arg_0_value = 5;"));
        assert!(program.contains("int __sg_arg_0_factor = 2;"));
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
            component: Some("fixture.divide".to_string()),
            source_path: PathBuf::new(),
            class_name: "DivideOps".to_string(),
            method_of: "DivideOps".to_string(),
            method_name: "Divide".to_string(),
            params: Vec::new(),
            return_type: "int".to_string(),
            return_nullable: false,
            exception_types: None,
            is_static: true,
        };
        let program = generate_csharp_program(&[&case], &[op], &[], &BTreeMap::new()).expect("program");

        assert!(program.contains("catch (System.Exception __ex)"));
        assert!(program.contains("SpecGateRuntime.EmitEvent(\"$fault\", __ex.Message);"));
    }

    #[test]
    fn metadata_resolution_picks_component_module_path() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path().join("Cargo.toml").as_path(),
            "[package]\nname = \"specgate-fixtures\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        );
        let src = tmp.path().join("src").join("engine").join("resolver_a.rs");
        write_file(
            src.as_path(),
            "#[spec_operation(\"render\")]\npub fn render() -> String { String::new() }\n",
        );
        let case = case_with_op("render");
        let cases = [&case];
        let registry = DiscoveryRegistry {
            operations: vec![
                DiscoveryOp {
                    name: "render".to_string(),
                    module_path: "specgate_fixtures::engine::resolver_b".to_string(),
                    component: "fixture.resolver_b".to_string(),
                    is_setup: false,
                },
                DiscoveryOp {
                    name: "render".to_string(),
                    module_path: "specgate_fixtures::engine::resolver_a".to_string(),
                    component: "fixture.resolver_a".to_string(),
                    is_setup: false,
                },
            ],
        };

        let resolved = metadata_fixture_sources(tmp.path(), "fixture.resolver_a", &cases, &registry)
            .expect("metadata resolution")
            .expect("metadata hit");

        assert_eq!(resolved, vec![src]);
    }

    #[test]
    fn metadata_resolution_errors_on_duplicate_component_operation() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path().join("Cargo.toml").as_path(),
            "[package]\nname = \"specgate-fixtures\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        );
        let case = case_with_op("render");
        let cases = [&case];
        let registry = DiscoveryRegistry {
            operations: vec![
                DiscoveryOp {
                    name: "render".to_string(),
                    module_path: "specgate_fixtures::engine::resolver_conflict".to_string(),
                    component: "fixture.resolver_conflict".to_string(),
                    is_setup: false,
                },
                DiscoveryOp {
                    name: "render".to_string(),
                    module_path: "specgate_fixtures::engine::resolver_conflict".to_string(),
                    component: "fixture.resolver_conflict".to_string(),
                    is_setup: false,
                },
            ],
        };

        let err = metadata_fixture_sources(tmp.path(), "fixture.resolver_conflict", &cases, &registry).expect_err("duplicate");

        assert_eq!(
            err,
            "operation 'render' is defined more than once in component 'fixture.resolver_conflict'"
        );
    }

    #[test]
    fn metadata_resolution_returns_none_when_component_has_no_operation() {
        let tmp = tempfile::tempdir().unwrap();
        write_file(
            tmp.path().join("Cargo.toml").as_path(),
            "[package]\nname = \"specgate-fixtures\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        );
        let case = case_with_op("render");
        let cases = [&case];
        let registry = DiscoveryRegistry {
            operations: vec![DiscoveryOp {
                name: "render".to_string(),
                module_path: "specgate_fixtures::engine::resolver_a".to_string(),
                component: "fixture.resolver_a".to_string(),
                is_setup: false,
            }],
        };

        let resolved = metadata_fixture_sources(tmp.path(), "fixture.legacy", &cases, &registry).expect("metadata resolution");

        assert!(resolved.is_none());
    }

    #[test]
    fn csharp_program_awaits_task_result_and_emits_unwrapped_value() {
        let mut inputs = BTreeMap::new();
        inputs.insert("url".to_string(), serde_yaml::Value::String("https://example.com".to_string()));
        let case = super::spec::Case {
            name: "fetch_returns_response".to_string(),
            target: None,
            operation: Some("fetch".to_string()),
            steps: Vec::new(),
            step_inputs: Vec::new(),
            inputs,
            expected: Vec::new(),
            level: super::CaseLevel::Must,
            source: None,
        };
        let op = CsOp {
            op_name: "fetch".to_string(),
            component: Some("fixture.async".to_string()),
            source_path: PathBuf::new(),
            class_name: "AsyncOps".to_string(),
            method_of: "AsyncOps".to_string(),
            method_name: "Fetch".to_string(),
            params: vec![("url".to_string(), "string".to_string())],
            return_type: "Task<string>".to_string(),
            return_nullable: false,
            exception_types: None,
            is_static: true,
        };

        let program = generate_csharp_program(&[&case], &[op], &[], &BTreeMap::new()).expect("program");

        assert!(program.contains("string __sg_result_0 = await AsyncOps.Fetch(__sg_arg_0_url);"));
        assert!(program.contains("SpecGateRuntime.EmitResult(__sg_result_0);"));
        assert!(!program.contains("Task<string> __sg_result_0 = AsyncOps.Fetch"));
    }

    #[test]
    fn csharp_program_awaits_void_like_task_without_result_event() {
        let case = super::spec::Case {
            name: "send".to_string(),
            target: None,
            operation: Some("send".to_string()),
            steps: Vec::new(),
            step_inputs: Vec::new(),
            inputs: BTreeMap::new(),
            expected: Vec::new(),
            level: super::CaseLevel::Must,
            source: None,
        };
        let op = CsOp {
            op_name: "send".to_string(),
            component: Some("fixture.async".to_string()),
            source_path: PathBuf::new(),
            class_name: "AsyncOps".to_string(),
            method_of: "AsyncOps".to_string(),
            method_name: "Send".to_string(),
            params: Vec::new(),
            return_type: "System.Threading.Tasks.ValueTask".to_string(),
            return_nullable: false,
            exception_types: None,
            is_static: true,
        };

        let program = generate_csharp_program(&[&case], &[op], &[], &BTreeMap::new()).expect("program");

        assert!(program.contains("await AsyncOps.Send();"));
        assert!(!program.contains("SpecGateRuntime.EmitResult(__sg_result_0);"));
    }

    #[test]
    fn csharp_async_return_parser_keeps_nested_generic_result_type() {
        let kind = super::csharp_return_kind("System.Threading.Tasks.ValueTask<Dictionary<string, List<int>>>").expect("return kind");

        match kind {
            super::CsReturnKind::AsyncValue(inner) => assert_eq!(inner, "Dictionary<string, List<int>>"),
            _ => panic!("expected async value"),
        }
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
            "FromSpecInput<Offset>(\"{\\\"dx\\\":1,\\\"dy\\\":2}\")"
        );
    }

    #[test]
    fn csharp_param_split_keeps_generic_commas() {
        assert_eq!(
            split_cs_params("[SpecInput(\"m\")] Dictionary<string,string> m, int count"),
            vec!["[SpecInput(\"m\")] Dictionary<string,string> m", "int count"]
        );
    }

    #[test]
    fn csharp_auto_property_parser_keeps_spaced_generic_type() {
        let prop = parse_csharp_auto_property("    public Dictionary<string, string> Attributes { get; set; } = new();").expect("property");

        assert_eq!(prop.visibility, "public");
        assert_eq!(prop.ty, "Dictionary<string, string>");
        assert_eq!(prop.name, "Attributes");
    }

    #[test]
    fn csharp_auto_property_parser_keeps_nested_nullable_array_type() {
        let prop = parse_csharp_auto_property("    public List<Dictionary<string, int?[]>>? Items { get; set; }").expect("property");

        assert_eq!(prop.ty, "List<Dictionary<string, int?[]>>?");
        assert_eq!(prop.name, "Items");
    }

    #[test]
    fn csharp_method_parser_keeps_spaced_generic_return_type() {
        let (_, _, return_type, is_static) =
            extract_cs_method_sig("public static Dictionary<string, string> Build(int count)").expect("method");

        assert_eq!(return_type, "Dictionary<string, string>");
        assert!(is_static);
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

    fn make_target(framework: Option<&str>, pkg: &Path) -> binding::Target {
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
        let target = make_target(Some("net8.0"), Path::new("."));
        assert_eq!(super::resolve_framework_for_runner(&target), "net8.0");
    }

    #[test]
    fn resolve_framework_falls_back_from_netstandard_in_binding() {
        let target = make_target(Some("netstandard2.0"), Path::new("."));
        assert_eq!(super::resolve_framework_for_runner(&target), "net10.0");
    }

    #[test]
    fn resolve_framework_reads_target_framework_from_csproj() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
    fn resolve_runner_settings_mirror_source_affecting_csproj_properties() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("target")
            .join("specgate-harness-unit-tests")
            .join(format!("runner_settings_test_{}", ID.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(
            scratch.join("Fixture.csproj"),
            "<Project><PropertyGroup><TargetFramework>net10.0</TargetFramework><Nullable>disable</Nullable><ImplicitUsings>enable</ImplicitUsings><LangVersion>latest</LangVersion></PropertyGroup></Project>",
        )
        .unwrap();
        let target = make_target(None, &scratch);

        let settings = super::resolve_csharp_runner_settings(&target);

        assert_eq!(settings.framework, "net10.0");
        assert_eq!(settings.nullable, "disable");
        assert_eq!(settings.implicit_usings, "enable");
        assert_eq!(settings.lang_version.as_deref(), Some("latest"));
    }

    #[test]
    fn resolve_framework_reads_first_target_frameworks_from_csproj() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static ID: AtomicU64 = AtomicU64::new(0);
        let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
        let scratch = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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
        let nonexistent = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("nonexistent")
            .join("path")
            .join("that")
            .join("does")
            .join("not")
            .join("exist");
        let target = make_target(None, &nonexistent);
        assert_eq!(super::resolve_framework_for_runner(&target), "net10.0");
    }

    #[test]
    fn test_extract_spec_operation_with_component() {
        let line = r#"[SpecOperation("add", Spec = "fixture.stateless_add")]"#;
        let result = super::extract_spec_operation_attr(line);
        assert_eq!(result, Some(("add".to_string(), Some("fixture.stateless_add".to_string()))));
    }

    #[test]
    fn test_extract_spec_operation_without_component() {
        let line = r#"[SpecOperation("add")]"#;
        let result = super::extract_spec_operation_attr(line);
        assert_eq!(result, Some(("add".to_string(), None)));
    }
}
