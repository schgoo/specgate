//! `SpecGate` harness — entry point.
//!
//! `run_spec(path)` loads a spec, locates the fixture source via the
//! binding, generates a temporary Cargo project that includes the
//! fixture and invokes its annotated functions, shells out to
//! `cargo run` to compile + execute, then reads emitted traces back
//! and subsequence-matches against each case's `expected:` list.
//!
//! The harness **never** parses or interprets the fixture source itself.
//! It only scans for attribute names and signatures (to validate the
//! spec references real symbols and to know how to call them), and
//! delegates everything else to the real Rust toolchain.

mod binding;
mod codegen;
mod match_traces;
pub(crate) mod scan;
mod spec;
mod types;

// Public API — what users need for run_spec() results
pub use types::{CaseLevel, CaseResult, CaseStatus, RunOutcome, Source};

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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

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
pub fn run_spec(spec_path: &str) -> RunOutcome {
    let path = PathBuf::from(spec_path);
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return RunOutcome::Error {
            reason: format!("spec file not found: {spec_path}"),
        };
    };

    // First: pure YAML validity.
    let yaml_value: serde_yaml::Value = match serde_yaml::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            return RunOutcome::Error {
                reason: "spec file is not valid YAML".into(),
            };
        }
    };

    // Then: spec shape parsing.
    let Ok(parsed) = spec::parse_spec(&yaml_value) else {
        return RunOutcome::Error {
            reason: "spec file is not valid YAML".into(),
        };
    };

    if parsed.cases.is_empty() {
        return RunOutcome::Error {
            reason: "spec has no test cases".into(),
        };
    }

    let Some(binding_path) = parsed.binding_path.as_deref() else {
        return RunOutcome::Error {
            reason: "spec has no binding".into(),
        };
    };
    let binding_full = binding_path_resolved(&path, binding_path);
    let Some(binding) = binding::load_binding(&binding_full) else {
        return RunOutcome::Error {
            reason: format!("binding '{binding_path}' not found"),
        };
    };

    // Shape check: spec-level event key validation.
    if let Some(reason) = check_shape(&parsed, &yaml_value) {
        return RunOutcome::Error { reason };
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

    // Validate every target exists before doing any IO-heavy work.
    for (eff_target, _) in &groups {
        let target_name = eff_target.as_deref();
        if binding.target(target_name).is_none() {
            return RunOutcome::Error {
                reason: format!("target '{}' not found in binding", target_name.unwrap_or("<default>")),
            };
        }
    }

    // Process each target group and accumulate results by original case index.
    let mut results_by_index: Vec<Option<CaseResult>> = vec![None; parsed.cases.len()];

    for (eff_target, case_indices) in &groups {
        let target = binding.target(eff_target.as_deref()).unwrap();
        let group_cases: Vec<&spec::Case> = case_indices.iter().map(|&i| &parsed.cases[i]).collect();

        // Give each target group a distinct scratch directory.
        let scratch_suffix = match eff_target.as_deref() {
            None => fixture_basename.clone(),
            Some(t) => format!("{fixture_basename}_{t}"),
        };
        let scratch_dir = scratch_for(&scratch_suffix);

        match run_group(target, &group_cases, &parsed, &fixture_basename, &workspace_root, &scratch_dir) {
            Ok(group_results) => {
                for (&case_idx, result) in case_indices.iter().zip(group_results) {
                    results_by_index[case_idx] = Some(result);
                }
            }
            Err(reason) => return RunOutcome::Error { reason },
        }
    }

    let results = results_by_index
        .into_iter()
        .map(|r| r.expect("all case indices covered by groups"))
        .collect();
    RunOutcome::Complete { results }
}

/// Run one target group: resolve source, validate annotations, generate a
/// temporary runner, compile + execute it, and return per-case results.
fn run_group(
    target: &binding::Target,
    group_cases: &[&spec::Case],
    spec: &spec::Spec,
    fixture_basename: &str,
    workspace_root: &Path,
    scratch_dir: &Path,
) -> Result<Vec<CaseResult>, String> {
    // Command target: run the binding's shell command once and map its exit
    // status to a synthetic `$outcome` event (exit 0 -> "Complete", else
    // "Error"), then match each case. No source file is resolved or compiled.
    if let Some(command) = target.command.as_deref() {
        return run_command_group(command, &target.package_root, group_cases);
    }

    let fixture_srcs = resolve_fixture_sources(&target.package_root, fixture_basename, group_cases);
    if fixture_srcs.is_empty() {
        if let Some(results) = short_circuit_non_must(group_cases, None) {
            return Ok(results);
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
        return Ok(build_short_circuit_results(group_cases, &case_disposition));
    }

    let cases_to_run: Vec<&spec::Case> = group_cases
        .iter()
        .zip(case_disposition.iter())
        .filter_map(|(&c, d)| matches!(d, CaseDisposition::Run).then_some(c))
        .collect();
    let needs_async = cases_to_run.iter().any(|c| case_uses_async(c, spec));

    let proj = codegen::generate(
        scratch_dir,
        &fixture_srcs,
        &codegen::GenerateConfig {
            spec,
            cases_to_run: &cases_to_run,
            annotated: &annotated,
            workspace_root,
            needs_async,
            fixture_pkg_root: Some(&target.package_root),
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

    let output = cmd.output().map_err(|e| format!("failed to invoke cargo: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("error[E") || stderr.contains("error:") || stderr.contains("could not compile") {
            return Err("source failed to compile".into());
        }
        return Err(format!("runner failed: {stderr}"));
    }

    let trace_text = std::fs::read_to_string(&proj.trace_file).map_err(|e| format!("runner produced no trace output: {e}"))?;
    let trace_map: std::collections::BTreeMap<String, Vec<TraceEvent>> =
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
            }),
            CaseDisposition::Warn => results.push(CaseResult {
                name: case.name.clone(),
                status: CaseStatus::Warn,
                level: case.level,
                source: case.source.clone(),
                expected: Vec::new(),
                traces: Vec::new(),
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
                });
            }
        }
    }
    Ok(results)
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

fn ops_metadata(raw: &serde_yaml::Value) -> std::collections::BTreeMap<String, OpMeta> {
    let mut out = std::collections::BTreeMap::new();
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
    use super::ops_metadata;

    fn meta(yaml: &str) -> std::collections::BTreeMap<String, super::OpMeta> {
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
}
