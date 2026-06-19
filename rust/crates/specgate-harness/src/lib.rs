//! SpecGate harness — entry point.
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
pub mod scan;
mod spec;
mod types;

pub use types::{
    AnyArg, AssertValue, Assertion, CaseLevel, CaseResult, CaseStatus, Matcher, RunOutcome, Source,
    TraceEvent, Value,
};

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn run_spec(spec_path: &str) -> RunOutcome {
    let path = PathBuf::from(spec_path);
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let raw = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => {
            return RunOutcome::Error {
                reason: format!("spec file not found: {spec_path}"),
            };
        }
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
    let parsed = match spec::parse_spec(&yaml_value) {
        Ok(s) => s,
        Err(_) => {
            return RunOutcome::Error {
                reason: "spec file is not valid YAML".into(),
            };
        }
    };

    if parsed.cases.is_empty() {
        return RunOutcome::Error {
            reason: "spec has no test cases".into(),
        };
    }

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

    let fixture_basename = spec_basename(&path);
    let fixture_src = match resolve_fixture_source(
        &binding.package_root,
        &fixture_basename,
        &parsed,
    ) {
        Some(p) => p,
        None => {
            // No source file matched. If every case is non-MUST, surface
            // skip/warn per case rather than failing the spec.
            if let Some(results) = short_circuit_non_must(&parsed.cases, None) {
                return RunOutcome::Complete { results };
            }
            return RunOutcome::Error {
                reason: format!(
                    "source file not found: {}",
                    binding
                        .package_root
                        .join("src")
                        .join(format!("{fixture_basename}.rs"))
                        .display()
                ),
            };
        }
    };
    let src_text = match std::fs::read_to_string(&fixture_src) {
        Ok(t) => t,
        Err(e) => {
            return RunOutcome::Error {
                reason: format!("source file unreadable: {} ({})", fixture_src.display(), e),
            };
        }
    };

    let annotated = scan::scan(&src_text);

    // Required setups + ops across all cases.
    let mut required_setups: BTreeSet<String> = BTreeSet::new();
    let mut required_ops: BTreeSet<String> = BTreeSet::new();
    for case in &parsed.cases {
        // Cases with a non-MUST level whose pieces are missing get
        // short-circuited later, so do not contribute to required sets.
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
            return RunOutcome::Error {
                reason: format!("setup '{s}' not found in source annotations"),
            };
        }
    }
    for o in &required_ops {
        if !annotated.operations.contains_key(o) {
            return RunOutcome::Error {
                reason: format!("operation '{o}' not found in source annotations"),
            };
        }
    }

    // Shape-check expected events against declared operation outputs.
    if let Some(reason) = check_shape(&parsed, &yaml_value) {
        return RunOutcome::Error { reason };
    }

    // Decide which cases will run via cargo vs short-circuit (skip/warn).
    let mut case_disposition: Vec<CaseDisposition> = Vec::with_capacity(parsed.cases.len());
    let mut runnable = false;
    for case in &parsed.cases {
        let disp = if case_pieces_available(case, &annotated) {
            runnable = true;
            CaseDisposition::Run
        } else {
            match case.level {
                CaseLevel::Must => {
                    // Already handled above by the required_* loops.
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
        // Every case is non-MUST and short-circuited. No cargo needed.
        let results = build_short_circuit_results(&parsed.cases, &case_disposition);
        return RunOutcome::Complete { results };
    }

    // Locate workspace root for path deps.
    let workspace_root = workspace_root();
    let scratch_dir = scratch_for(&fixture_basename);

    // Determine if ANY runnable case uses an async op — drives async runtime
    // scaffolding in the generated runner.
    let cases_to_run: Vec<&spec::Case> = parsed
        .cases
        .iter()
        .zip(case_disposition.iter())
        .filter_map(|(c, d)| matches!(d, CaseDisposition::Run).then_some(c))
        .collect();
    let needs_async = cases_to_run.iter().any(|c| case_uses_async(c, &parsed));

    let proj = match codegen::generate(
        &scratch_dir,
        &fixture_src,
        &parsed,
        &cases_to_run,
        &annotated,
        &workspace_root,
        needs_async,
        &binding.package_root,
    ) {
        Ok(p) => p,
        Err(e) => {
            return RunOutcome::Error {
                reason: format!("failed to scaffold runner: {e}"),
            };
        }
    };

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
    // Default to offline if not explicitly overridden — git deps are already
    // cached by the parent workspace build, and crates.io is often blocked
    // in restricted sandboxes.
    if std::env::var_os("CARGO_NET_OFFLINE").is_none() {
        cmd.env("CARGO_NET_OFFLINE", "true");
    }
    cmd.env(
        "CARGO_TARGET_DIR",
        proj.crate_dir.join("target").as_os_str(),
    );

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return RunOutcome::Error {
                reason: format!("failed to invoke cargo: {e}"),
            };
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("error[E") || stderr.contains("error:") || stderr.contains("could not compile") {
            return RunOutcome::Error {
                reason: "source failed to compile".into(),
            };
        }
        return RunOutcome::Error {
            reason: format!("runner failed: {}", stderr),
        };
    }

    // Load traces.
    let trace_text = match std::fs::read_to_string(&proj.trace_file) {
        Ok(t) => t,
        Err(_) => {
            return RunOutcome::Error {
                reason: "runner produced no trace output".into(),
            };
        }
    };
    let trace_map: std::collections::BTreeMap<String, Vec<TraceEvent>> =
        match serde_yaml::from_str(&trace_text) {
            Ok(m) => m,
            Err(e) => {
                return RunOutcome::Error {
                    reason: format!("failed to parse traces: {e}"),
                };
            }
        };

    let mut results = Vec::with_capacity(parsed.cases.len());
    for (case, disp) in parsed.cases.iter().zip(case_disposition.iter()) {
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
    RunOutcome::Complete { results }
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
fn short_circuit_non_must(
    cases: &[spec::Case],
    _annotated: Option<&scan::AnnotatedSource>,
) -> Option<Vec<CaseResult>> {
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

fn build_short_circuit_results(
    cases: &[spec::Case],
    disp: &[CaseDisposition],
) -> Vec<CaseResult> {
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
    // CARGO_MANIFEST_DIR for specgate-harness is .../rust/crates/specgate-harness.
    // Walk up to .../rust.
    let mut p = PathBuf::from(env_or("CARGO_MANIFEST_DIR", "."));
    p.pop(); // specgate-harness
    p.pop(); // crates
    p
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
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
/// most setups + operations the spec needs.
fn resolve_fixture_source(
    package_root: &Path,
    fixture_basename: &str,
    spec: &spec::Spec,
) -> Option<PathBuf> {
    let direct = package_root.join("src").join(format!("{fixture_basename}.rs"));
    if direct.exists() {
        return Some(direct);
    }
    // Build required sets.
    let mut req_setups: BTreeSet<String> = BTreeSet::new();
    let mut req_ops: BTreeSet<String> = BTreeSet::new();
    for case in &spec.cases {
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
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if fixture_basename.starts_with(stem) && stem.len() > 4 {
                score += stem.len();
            }
        }
        if best.as_ref().map(|b| score > b.0).unwrap_or(true) && score > 0 {
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
                        .map(|m| {
                            !m.outputs.is_empty()
                                && m.outputs.iter().all(|o| o.starts_with(&format!("{op}.")))
                        })
                        .unwrap_or(false)
                });
                if let Some(op) = strict_op {
                    return Some(format!(
                        "expected event '{k}' is not a declared output of operation '{op}'"
                    ));
                }
            }
        }
    }
    None
}

fn collect_event_names(a: &types::Assertion, out: &mut Vec<String>) {
    match a {
        types::Assertion::Event { name, .. } => out.push(name.clone()),
        types::Assertion::Run { .. } => {}
        types::Assertion::Unordered { items } | types::Assertion::Anywhere { items } => {
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
    let map = match raw.as_mapping() {
        Some(m) => m,
        None => return out,
    };
    let ops = match map.get(serde_yaml::Value::String("operations".into())) {
        Some(serde_yaml::Value::Mapping(m)) => m,
        _ => return out,
    };
    for (k, v) in ops {
        let name = match k.as_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let body = match v.as_mapping() {
            Some(m) => m,
            None => continue,
        };
        let mut meta = OpMeta::default();
        if let Some(serde_yaml::Value::Mapping(inputs)) =
            body.get(serde_yaml::Value::String("inputs".into()))
        {
            for (ik, _) in inputs {
                if let Some(s) = ik.as_str() {
                    meta.inputs.push(s.to_string());
                }
            }
        }
        if let Some(serde_yaml::Value::Sequence(outs)) =
            body.get(serde_yaml::Value::String("outputs".into()))
        {
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
