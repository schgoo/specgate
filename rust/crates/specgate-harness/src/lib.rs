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
    let binding_full = spec::binding_path_resolved(&path, binding_path);
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
    let Some(fixture_src) = resolve_fixture_source(&target.package_root, fixture_basename, group_cases) else {
        if let Some(results) = short_circuit_non_must(group_cases, None) {
            return Ok(results);
        }
        return Err(format!(
            "source file not found: {}",
            target.package_root.join("src").join(format!("{fixture_basename}.rs")).display()
        ));
    };

    let src_text =
        std::fs::read_to_string(&fixture_src).map_err(|e| format!("source file unreadable: {} ({})", fixture_src.display(), e))?;

    let annotated = scan::scan(&src_text);

    // Required setups + ops across all MUST cases in this group.
    let mut required_setups: BTreeSet<String> = BTreeSet::new();
    let mut required_ops: BTreeSet<String> = BTreeSet::new();
    for case in group_cases {
        if case.level != CaseLevel::Must && !case_pieces_available(case, &annotated) {
            continue;
        }
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
    for s in &required_setups {
        if !annotated.setups.contains_key(s) {
            return Err(format!("setup '{s}' not found in source annotations"));
        }
    }
    for o in &required_ops {
        if !annotated.operations.contains_key(o) {
            return Err(format!("operation '{o}' not found in source annotations"));
        }
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
        &fixture_src,
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
// Per-case classification helpers
// ---------------------------------------------------------------------------

enum CaseDisposition {
    Run,
    Skip,
    Warn,
}

fn case_pieces_available(case: &spec::Case, annotated: &scan::AnnotatedSource) -> bool {
    match &case.setup {
        spec::Setup::None => {}
        spec::Setup::Single(n) => {
            if !annotated.setups.contains_key(n) {
                return false;
            }
        }
        spec::Setup::Multi(entries) => {
            for (_, n) in entries {
                if !annotated.setups.contains_key(n) {
                    return false;
                }
            }
        }
    }
    let ops: Vec<&str> = if !case.steps.is_empty() {
        case.steps.iter().map(String::as_str).collect()
    } else if let Some(op) = case.operation.as_deref() {
        vec![op]
    } else {
        return true;
    };
    ops.iter().all(|o| annotated.operations.contains_key(*o))
}

/// If every case has level != MUST, return per-case warn/skip results.
fn short_circuit_non_must(cases: &[&spec::Case], _annotated: Option<&scan::AnnotatedSource>) -> Option<Vec<CaseResult>> {
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

/// Try to find the fixture source file for a spec. Prefer `<basename>.rs`;
/// otherwise pick the .rs file under src/ whose annotations contain the
/// most setups + operations the cases need.
fn resolve_fixture_source(package_root: &Path, fixture_basename: &str, cases: &[&spec::Case]) -> Option<PathBuf> {
    let direct = package_root.join("src").join(format!("{fixture_basename}.rs"));
    if direct.exists() {
        return Some(direct);
    }
    // Build required sets from the provided cases.
    let mut req_setups: BTreeSet<String> = BTreeSet::new();
    let mut req_ops: BTreeSet<String> = BTreeSet::new();
    for case in cases {
        match &case.setup {
            spec::Setup::None => {}
            spec::Setup::Single(n) => {
                req_setups.insert(n.clone());
            }
            spec::Setup::Multi(es) => {
                for (_, n) in es {
                    req_setups.insert(n.clone());
                }
            }
        }
        if !case.steps.is_empty() {
            for s in &case.steps {
                req_ops.insert(s.clone());
            }
        } else if let Some(op) = case.operation.as_deref() {
            req_ops.insert(op.to_string());
        }
    }

    let src_dir = package_root.join("src");
    let entries = std::fs::read_dir(&src_dir).ok()?;
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let annotated = scan::scan(&text);
        let mut score = 0usize;
        for o in &req_ops {
            if annotated.operations.contains_key(o) {
                score += 2;
            }
        }
        for s in &req_setups {
            if annotated.setups.contains_key(s) {
                score += 1;
            }
        }
        // Light filename-similarity bonus.
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            && fixture_basename.starts_with(stem)
            && stem.len() > 4
        {
            score += stem.len();
        }
        if best.as_ref().is_none_or(|b| score > b.0) && score > 0 {
            best = Some((score, path));
        }
    }
    best.map(|(_, p)| p)
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
        let case_setups: Vec<&str> = match &case.setup {
            spec::Setup::None => vec![],
            spec::Setup::Single(n) => vec![n.as_str()],
            spec::Setup::Multi(entries) => entries.iter().map(|(_, fn_name)| fn_name.as_str()).collect(),
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
        }
        for setup in &case_setups {
            if let Some(meta) = ops_meta.get(*setup) {
                for inp in &meta.inputs {
                    allowed.insert(format!("{setup}.{inp}"));
                }
            }
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
                let strict_op = case_ops.iter().find(|op| {
                    ops_meta
                        .get(**op)
                        .is_some_and(|m| !m.outputs.is_empty() && m.outputs.iter().all(|o| o.starts_with(&format!("{op}."))))
                });
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
                if let Some(s) = o.as_str() {
                    meta.outputs.push(s.to_string());
                }
            }
        }
        out.insert(name, meta);
    }
    out
}
