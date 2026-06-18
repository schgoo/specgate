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

pub use types::{CaseResult, CaseStatus, RunOutcome, TraceEvent};

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    let parsed = match spec::parse_value(&yaml_value) {
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
        Err(_) => {
            return RunOutcome::Error {
                reason: "source failed to compile".into(),
            };
        }
    };

    let annotated = scan::scan(&src_text);

    // Required setups + ops across all cases.
    let mut required_setups: BTreeSet<String> = BTreeSet::new();
    let mut required_ops: BTreeSet<String> = BTreeSet::new();
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

    // Locate workspace root for path deps.
    let workspace_root = workspace_root();
    let scratch_dir = scratch_for(&fixture_basename);

    let proj = match codegen::generate(
        &scratch_dir,
        &fixture_src,
        &parsed,
        &annotated,
        &workspace_root,
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
        .arg("--offline")
        .arg("--manifest-path")
        .arg(proj.crate_dir.join("Cargo.toml"))
        .arg("--")
        .arg(&proj.trace_file);
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
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
        // Heuristic: if rustc/cargo reports a compile error, surface
        // "source failed to compile". Other failures (panic at runtime)
        // are unexpected — surface their message.
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
    for case in &parsed.cases {
        let traces = trace_map.get(&case.name).cloned().unwrap_or_default();
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
        }
        for setup in &case_setups {
            if let Some(meta) = ops_meta.get(*setup) {
                for inp in &meta.inputs {
                    allowed.insert(format!("{setup}.{inp}"));
                }
            }
        }

        for entry in &case.expected {
            if entry.len() != 1 {
                continue;
            }
            let (k, _) = entry.iter().next().unwrap();
            if k == "run" {
                continue;
            }
            if allowed.contains(k) {
                continue;
            }
            // Any event prefixed with one of the case's op names is
            // accepted (the matcher itself decides whether it actually
            // appeared in traces).
            if case_ops.iter().any(|op| k.starts_with(&format!("{op}."))) {
                continue;
            }
            // Strict-mode ops have ALL declared outputs prefixed with
            // `<op>.`. If no op in this case is strict-mode, skip the
            // shape check entirely — bare field events (state-machine
            // style) are allowed to be anything.
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
    None
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
