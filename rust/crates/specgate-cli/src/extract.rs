//! `specgate extract <package_root> -o <out>` — deterministically derive a
//! spec skeleton from an annotated Rust crate.
//!
//! Extraction reads the crate's *link-time* operation/type registry (the
//! `#[spec_operation]` / `#[spec_setup]` / `SpecEvent` metadata collected via
//! `linkme`) rather than parsing source or invoking an LLM. A tiny discovery
//! binary is scaffolded that depends on the target crate, calls
//! `specgate::__rt::discovery_json()`, and prints the registry as JSON; the
//! JSON is then mapped to a `.spec.yaml` skeleton plus a sibling binding file.
//!
//! By default only the schema (operations, inputs/outputs, types) is derived,
//! leaving `cases:` empty — so a freshly-extracted skeleton validates as sound
//! except for the expected `no_cases` finding. With `--cases`, the crate's
//! existing tests are run under record mode and each passing test is captured
//! as a case, producing a complete, runnable spec.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use specgate::{SpecEvent, TraceEvent, Value, spec_operation};
use specgate_harness::discovery::{
    OpInfo, Registry, SpecType, TypeInfo, build_inputs, cargo_bin, cargo_package_name, collect_named_refs, is_unit, map_type, raw_inputs,
    run_discovery, to_cargo_path, workspace_root,
};

/// Summary of an extraction run.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub struct ExtractReport {
    #[spec_event]
    pub spec_name: String,
    #[spec_event]
    pub operations: i32,
    #[spec_event]
    pub types: i32,
    #[spec_event]
    pub cases: i32,
    #[spec_event]
    pub output_path: String,
}

/// Outcome of `extract`.
#[derive(Debug, Clone, PartialEq, Eq, SpecEvent)]
pub enum ExtractOutcome {
    Complete { report: ExtractReport },
    Error { reason: String },
}

impl std::fmt::Display for ExtractOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractOutcome::Complete { report } => write!(
                f,
                "Complete(spec={}, operations={}, types={}, cases={}, output={})",
                report.spec_name, report.operations, report.types, report.cases, report.output_path
            ),
            ExtractOutcome::Error { reason } => write!(f, "Error({reason})"),
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Extract a spec skeleton from the annotated crate at
/// `package_root`, writing the `.spec.yaml` to `out` and a sibling binding file.
///
/// Returns [`ExtractOutcome::Error`] (never panics) when `package_root` does not
/// exist, is not a Rust crate (no `Cargo.toml`), the discovery build fails, or
/// the output cannot be written.
#[must_use]
#[spec_operation("extract")]
pub fn extract(package_root: &str, out: &str, component: &str, cases: bool) -> ExtractOutcome {
    let root = Path::new(package_root);
    if !root.exists() {
        return ExtractOutcome::Error {
            reason: format!("package_root not found: {package_root}"),
        };
    }
    if !root.join("Cargo.toml").is_file() {
        return ExtractOutcome::Error {
            reason: format!("not a Rust crate (no Cargo.toml): {package_root}"),
        };
    }

    let json = match run_discovery(root) {
        Ok(j) => j,
        Err(reason) => return ExtractOutcome::Error { reason },
    };

    let registry = match Registry::parse(&json) {
        Ok(r) => r,
        Err(reason) => return ExtractOutcome::Error { reason },
    };

    let components = registry.present_components();
    let selected = if component.is_empty() {
        match components.len() {
            0 => {
                return ExtractOutcome::Error {
                    reason: "no components found: crate has no annotated operations or types".to_string(),
                };
            }
            1 => components[0].clone(),
            _ => {
                return ExtractOutcome::Error {
                    reason: format!(
                        "multiple components present ({}); select one with --component <name>",
                        components.join(", ")
                    ),
                };
            }
        }
    } else if components.iter().any(|c| c == component) {
        component.to_string()
    } else {
        return ExtractOutcome::Error {
            reason: format!("component '{component}' not found; available components: {}", components.join(", ")),
        };
    };

    let depends_on = match resolve_dependencies(&registry, &selected) {
        Ok(d) => d,
        Err(reason) => return ExtractOutcome::Error { reason },
    };

    // Capture test cases by running the crate's existing tests under record
    // mode. Schema-only extraction (the default) leaves this empty.
    let captured = if cases {
        match capture_cases(root, &registry, &selected) {
            Ok(c) => c,
            Err(reason) => return ExtractOutcome::Error { reason },
        }
    } else {
        Vec::new()
    };

    let spec_name = selected.clone();
    let out_path = Path::new(out);
    let binding_file_name = binding_file_name(out_path);
    let spec_yaml = render_spec(&spec_name, &binding_file_name, &registry, &selected, &depends_on, &captured, cases);

    // Create the output directory first so the binding's relative package_root
    // can be computed against a path that exists on disk (canonicalize requires
    // the directory to be present, otherwise it falls back to a verbatim path).
    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return ExtractOutcome::Error {
            reason: format!("failed to create output directory {}: {e}", parent.display()),
        };
    }

    // The binding's package_root is relative to the binding file (= out's dir).
    let out_dir = out_path.parent().unwrap_or_else(|| Path::new("."));
    let rel_pkg = relative_path(out_dir, root);
    let binding_yaml = render_binding(&rel_pkg);

    if let Err(e) = std::fs::write(out_path, &spec_yaml) {
        return ExtractOutcome::Error {
            reason: format!("failed to write spec to {out}: {e}"),
        };
    }
    let binding_path = out_dir.join(&binding_file_name);
    if let Err(e) = std::fs::write(&binding_path, &binding_yaml) {
        return ExtractOutcome::Error {
            reason: format!("failed to write binding to {}: {e}", binding_path.display()),
        };
    }

    let report = ExtractReport {
        spec_name,
        operations: i32::try_from(registry.operations_for(&selected).len()).unwrap_or(i32::MAX),
        types: i32::try_from(registry.local_types(&selected).len()).unwrap_or(i32::MAX),
        cases: i32::try_from(captured.len()).unwrap_or(i32::MAX),
        output_path: out.to_string(),
    };
    ExtractOutcome::Complete { report }
}

// The discovery build, registry parse, type normalization, and invisible-setup
// folding live in `specgate_harness::discovery` (shared with the harness's
// `discover` operation); they are imported at the top of this module.

// ---------------------------------------------------------------------------
// Case capture — run the crate's existing tests under record mode and
// reconstruct each PASSING test as a spec case.
// ---------------------------------------------------------------------------

/// One observed operation invocation within a captured test. `inputs` are the
/// param name/value pairs recovered from the `<op>.<param>` echo events, in
/// emission order (= declared param order for free functions).
#[derive(Debug, Clone)]
struct CaseInvocation {
    operation: String,
    inputs: Vec<(String, String)>,
}

/// A captured test turned into a spec case. `expected` is the filtered
/// trace the test emitted (`$run` + input echoes + `$result`, per invocation);
/// `setup` holds construction inputs recovered from `$setup.<key>` record-only
/// events (empty for free-function cases).
#[derive(Debug, Clone)]
struct CaseData {
    name: String,
    setup: BTreeMap<String, String>,
    invocations: Vec<CaseInvocation>,
    expected: Vec<TraceEvent>,
}

/// Build the target crate's test binary, enumerate its tests, run each one in
/// isolation under record mode, and reconstruct the passing tests as cases for
/// `comp`. Free-function ops and method ops are both eligible once their params
/// are echoed (A2 ensures method params emit just like free-function params).
fn capture_cases(package_root: &Path, registry: &Registry, comp: &str) -> Result<Vec<CaseData>, String> {
    let crate_name = cargo_package_name(package_root).ok_or_else(|| "could not read crate name from Cargo.toml".to_string())?;
    let scratch = workspace_root().join("target").join("specgate-extract-cases").join(&crate_name);
    std::fs::create_dir_all(&scratch).map_err(|e| format!("failed to create case-capture scratch dir: {e}"))?;

    let test_bin = build_test_binary(package_root, &scratch)?;
    let test_names = list_tests(&test_bin)?;

    let mut raw: Vec<(String, Vec<TraceEvent>)> = Vec::new();
    for name in &test_names {
        let record_file = scratch.join(format!("{}.jsonl", sanitize_file_stem(name)));
        let _ = std::fs::remove_file(&record_file);
        let passed = run_recorded_test(&test_bin, name, &record_file)?;
        if !passed {
            // A failing test is not a valid conformance case.
            continue;
        }
        let events = read_record(&record_file)?;
        raw.push((name.clone(), events));
    }

    Ok(build_cases(raw, registry, comp))
}

/// `cargo test --no-run` in the crate, returning the path to the compiled test
/// executable (preferring the library's unit-test binary).
fn build_test_binary(package_root: &Path, scratch: &Path) -> Result<PathBuf, String> {
    let mut cmd = Command::new(cargo_bin());
    cmd.arg("test").arg("--no-run").arg("--quiet").arg("--message-format=json");
    cmd.current_dir(package_root);
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
    cmd.env("CARGO_TARGET_DIR", scratch.join("target").as_os_str());

    let output = cmd.output().map_err(|e| format!("failed to build test binary: {e}"))?;
    if !output.status.success() {
        return Err(format!("test build failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut fallback: Option<PathBuf> = None;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let is_test = v
            .get("profile")
            .and_then(|p| p.get("test"))
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !is_test {
            continue;
        }
        let Some(exe) = v.get("executable").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let is_lib = v
            .get("target")
            .and_then(|t| t.get("kind"))
            .and_then(serde_json::Value::as_array)
            .is_some_and(|ks| ks.iter().any(|k| k.as_str() == Some("lib")));
        if is_lib {
            return Ok(PathBuf::from(exe));
        }
        fallback.get_or_insert_with(|| PathBuf::from(exe));
    }
    fallback.ok_or_else(|| "no test binary was produced (crate has no tests?)".to_string())
}

/// Enumerate a libtest binary's test cases via `--list`. Returns the fully
/// qualified test names (e.g. `tests::adds_several`).
fn list_tests(test_bin: &Path) -> Result<Vec<String>, String> {
    let output = Command::new(test_bin)
        .arg("--list")
        .arg("--format")
        .arg("terse")
        .output()
        .map_err(|e| format!("failed to list tests: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "test enumeration failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut names = Vec::new();
    for line in stdout.lines() {
        if let Some(name) = line.strip_suffix(": test") {
            names.push(name.trim().to_string());
        }
    }
    Ok(names)
}

/// Run one test in isolation with record mode enabled. Returns whether it
/// passed (exit success).
fn run_recorded_test(test_bin: &Path, test_name: &str, record_file: &Path) -> Result<bool, String> {
    let output = Command::new(test_bin)
        .arg(test_name)
        .arg("--exact")
        .arg("--test-threads=1")
        .env("SPECGATE_RECORD", record_file)
        .output()
        .map_err(|e| format!("failed to run test '{test_name}': {e}"))?;
    Ok(output.status.success())
}

/// Parse a per-test JSONL record file into a trace. A missing file means the
/// test emitted nothing (no annotated ops).
fn read_record(path: &Path) -> Result<Vec<TraceEvent>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("failed to read record file {}: {e}", path.display()))?;
    let mut events = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let ev: TraceEvent = serde_json::from_str(line).map_err(|e| format!("failed to parse recorded event: {e}"))?;
        events.push(ev);
    }
    Ok(events)
}

/// Split a test's trace into invocations by `Run` events, recovering each
/// invocation's inputs from its `<op>.<param>` echo events.
fn segment_invocations(events: &[TraceEvent]) -> Vec<CaseInvocation> {
    let mut invs: Vec<CaseInvocation> = Vec::new();
    for ev in events {
        match ev {
            TraceEvent::Run { operation } => invs.push(CaseInvocation {
                operation: operation.clone(),
                inputs: Vec::new(),
            }),
            TraceEvent::Event { name, value } => {
                if let Some(cur) = invs.last_mut() {
                    let prefix = format!("{}.", cur.operation);
                    if let Some(param) = name.strip_prefix(&prefix) {
                        cur.inputs.push((param.to_string(), value_as_string(value)));
                    }
                }
            }
        }
    }
    invs
}

/// Split a test's trace into a setup map and invocations. `$setup.<key>`
/// events written by `#[spec_setup]` echo statements are consumed into the
/// map (key = part after `$setup.`); remaining events are forwarded to
/// [`segment_invocations`].
fn segment_trace(events: &[TraceEvent]) -> (BTreeMap<String, String>, Vec<CaseInvocation>) {
    let mut setup: BTreeMap<String, String> = BTreeMap::new();
    for ev in events {
        if let TraceEvent::Event { name, value } = ev
            && let Some(key) = name.strip_prefix("$setup.")
        {
            setup.insert(key.to_string(), value_as_string(value));
        }
    }
    (setup, segment_invocations(events))
}

/// Filter a test's raw trace down to only the events that belong in
/// `expected:`: `Run` events, `$result` events, and per-param echo events
/// (`<op>.<param>` for any non-setup operation of `comp`). Discards
/// `$setup.*` events and bare field-mutation events emitted by the
/// `BodyInstrumenter`.
fn filter_expected(events: &[TraceEvent], registry: &Registry, comp: &str) -> Vec<TraceEvent> {
    let op_prefixes: Vec<String> = registry
        .ops
        .iter()
        .filter(|o| !o.is_setup && o.component == comp)
        .map(|o| format!("{}.", o.name))
        .collect();
    events
        .iter()
        .filter(|ev| match ev {
            TraceEvent::Run { .. } => true,
            TraceEvent::Event { name, .. } => name == "$result" || op_prefixes.iter().any(|p| name.starts_with(p.as_str())),
        })
        .cloned()
        .collect()
}

/// True when every op in `inv` is a non-setup operation of `comp` with all its
/// declared params echoed. After A2, method ops also emit param echoes, so this
/// check accepts both free functions and methods once their params are recorded.
fn is_free_fn_invocation(inv: &CaseInvocation, registry: &Registry, comp: &str) -> bool {
    let Some(op) = registry
        .ops
        .iter()
        .find(|o| !o.is_setup && o.name == inv.operation && o.component == comp)
    else {
        return false;
    };
    let echoed: BTreeSet<&str> = inv.inputs.iter().map(|(k, _)| k.as_str()).collect();
    op.params.iter().all(|(pn, _)| echoed.contains(pn.as_str()))
}

/// Pending eligible case data collected before name-dedup.
type EligibleCase = (String, BTreeMap<String, String>, Vec<CaseInvocation>, Vec<TraceEvent>);

/// Reconstruct eligible passing tests as cases: skip empty tests and tests
/// where any invocation has un-echoed params; name each case after its test
/// (qualifying collisions with the module path); order alphabetically.
fn build_cases(raw: Vec<(String, Vec<TraceEvent>)>, registry: &Registry, comp: &str) -> Vec<CaseData> {
    let mut eligible: Vec<EligibleCase> = Vec::new();
    for (test_name, events) in raw {
        let (setup, invs) = segment_trace(&events);
        if invs.is_empty() {
            // Test exercised no annotated operations — nothing to capture.
            continue;
        }
        if !invs.iter().all(|inv| is_free_fn_invocation(inv, registry, comp)) {
            eprintln!("extract --cases: skipping test '{test_name}' (some invocations have un-echoed params)");
            continue;
        }
        let expected = filter_expected(&events, registry, comp);
        eligible.push((test_name, setup, invs, expected));
    }

    // Bare (module-stripped) sanitized names, and how often each recurs.
    let bare: Vec<String> = eligible.iter().map(|(tn, _, _, _)| sanitize_case_name(last_segment(tn))).collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for b in &bare {
        *counts.entry(b.clone()).or_default() += 1;
    }

    let mut cases: Vec<CaseData> = Vec::new();
    for ((test_name, setup, invocations, expected), b) in eligible.into_iter().zip(bare) {
        // Qualify with the full module path only when the bare name collides.
        let name = if counts.get(&b).copied().unwrap_or(0) > 1 {
            sanitize_case_name(&test_name.replace("::", "_"))
        } else {
            b
        };
        cases.push(CaseData {
            name,
            setup,
            invocations,
            expected,
        });
    }
    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}

/// The last `::`-delimited segment of a test's fully qualified name.
fn last_segment(full: &str) -> &str {
    full.rsplit("::").next().unwrap_or(full)
}

/// Sanitize a name to the case-name pattern `^[a-z][a-z0-9_]*$`: lowercase,
/// non-alphanumeric/underscore chars become `_`, and a leading non-letter is
/// prefixed with `c`.
fn sanitize_case_name(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if !out.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        out.insert(0, 'c');
    }
    out
}

/// Sanitize a test name into a filesystem-safe record-file stem.
fn sanitize_file_stem(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

/// The raw string an event carries (echoes are always `Value::String`); other
/// shapes fall back to their JSON form.
fn value_as_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Spec-name derivation
// ---------------------------------------------------------------------------

/// Binding file name beside the spec: `<stem>.binding.yaml`, where `<stem>` is
/// the spec file name with a trailing `.spec.yaml` / `.yaml` removed.
fn binding_file_name(out: &Path) -> String {
    let file = out.file_name().and_then(|n| n.to_str()).unwrap_or("extracted.spec.yaml");
    let stem = file
        .strip_suffix(".spec.yaml")
        .or_else(|| file.strip_suffix(".yaml"))
        .unwrap_or(file);
    format!("{stem}.binding.yaml")
}

/// Compute a relative path from `from_dir` to `to` using canonicalized absolute
/// paths, falling back to the original `to` if either cannot be resolved.
fn relative_path(from_dir: &Path, to: &Path) -> String {
    let from = std::fs::canonicalize(from_dir).ok();
    let to_abs = std::fs::canonicalize(to).ok();
    let (Some(from), Some(to_abs)) = (from, to_abs) else {
        return to_cargo_path(to);
    };
    let from_comps: Vec<_> = from.components().collect();
    let to_comps: Vec<_> = to_abs.components().collect();
    let common = from_comps.iter().zip(to_comps.iter()).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from_comps.len() {
        parts.push("..".to_string());
    }
    for c in &to_comps[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

// ---------------------------------------------------------------------------
// Type-ref mapping — the SpecType/RustType model, `map_type`, `is_builtin_type`,
// `collect_named_refs`, and the Rust-type parser now live in
// `specgate_harness::discovery` (imported at the top of this module).
// ---------------------------------------------------------------------------

/// Every (named-type, location) reference on the selected component's spec
/// surface: operation inputs/outputs and the component's own type fields/variants.
fn referenced_types(registry: &Registry, comp: &str) -> Vec<(String, String)> {
    let mut refs: Vec<(String, String)> = Vec::new();
    for op in registry.operations_for(comp) {
        for (_n, ty) in raw_inputs(op, registry) {
            let mut names = Vec::new();
            collect_named_refs(&ty, &mut names);
            for nm in names {
                refs.push((nm, format!("operation '{}' input", op.name)));
            }
        }
        if !is_unit(&op.return_type) {
            let mut names = Vec::new();
            collect_named_refs(&op.return_type, &mut names);
            for nm in names {
                refs.push((nm, format!("operation '{}' output", op.name)));
            }
        }
    }
    for t in registry.local_types(comp) {
        for (fname, fty) in &t.fields {
            let mut names = Vec::new();
            collect_named_refs(fty, &mut names);
            for nm in names {
                refs.push((nm, format!("type '{}' field '{}'", t.name, fname)));
            }
        }
        for v in &t.variants {
            for (fname, fty) in &v.fields {
                let mut names = Vec::new();
                collect_named_refs(fty, &mut names);
                for nm in names {
                    refs.push((nm, format!("type '{}' variant '{}' field '{}'", t.name, v.name, fname)));
                }
            }
        }
    }
    refs
}

/// Resolve the selected component's referenced named types into a sorted,
/// de-duplicated `depends_on` list (the owning components of FOREIGN types).
/// Hard-errors on any reference that is neither a registered `SpecEvent` type nor
/// a known primitive/collection (the no-dynamic-types rule).
fn resolve_dependencies(registry: &Registry, comp: &str) -> Result<Vec<String>, String> {
    let type_comp: BTreeMap<&str, &str> = registry.types.iter().map(|t| (t.name.as_str(), t.component.as_str())).collect();
    let mut deps: BTreeSet<String> = BTreeSet::new();
    for (name, location) in referenced_types(registry, comp) {
        match type_comp.get(name.as_str()) {
            Some(&owner) if owner == comp => {}
            Some(&owner) => {
                deps.insert(owner.to_string());
            }
            None => {
                return Err(format!(
                    "unresolved type '{name}' referenced by {location}: not a registered SpecEvent type or known primitive/collection"
                ));
            }
        }
    }
    deps.remove(comp);
    Ok(deps.into_iter().collect())
}

// The Rust-type parser (`parse_rust_type`, `tokenize_type`, `parse_type_tokens`,
// `normalize_type`) and the setup-aware input folding (`raw_inputs`,
// `build_inputs`) now live in `specgate_harness::discovery` (imported at the top
// of this module).

// ---------------------------------------------------------------------------
// YAML emission
// ---------------------------------------------------------------------------

const SPEC_HEADER: &str = "# Spec skeleton extracted from your code by `specgate extract`.\n\
#\n\
# This captures the schema (types + operations) of the annotated crate but has\n\
# no test cases yet. Add `cases:` with expected assertions to make it runnable,\n\
# then check your code against it with `specgate run <this-file>`. Edit freely;\n\
# regenerate the schema anytime with `specgate extract <package_root> -o <out>`.\n\
#\n\
# Docs: https://github.com/schgoo/specgate\n";

const CASES_HEADER: &str = "# Spec extracted from your code by `specgate extract --cases`.\n\
#\n\
# Captures the schema (types + operations) AND test cases observed by running\n\
# your existing tests: each case mirrors a test — its recovered inputs and the\n\
# full trace the operation emitted ($run, inputs, $result). Edit freely;\n\
# regenerate with `specgate extract <package_root> --cases -o <out>`.\n\
#\n\
# Docs: https://github.com/schgoo/specgate\n";

const BINDING_HEADER: &str = "# Binding extracted by `specgate extract`: links this spec to the code under\n\
# test. Point `package_root` at the crate you want to run the spec against.\n\
#\n\
# Docs: https://github.com/schgoo/specgate\n";

/// Quote a scalar reference string when it contains generic-bracket syntax that
/// would otherwise be ambiguous as an emitted spec type (`Option<T>`, etc.).
fn quote_ref(s: &str) -> String {
    if s.contains('<') { format!("\"{s}\"") } else { s.to_string() }
}

/// Render the full spec skeleton YAML. When `cases_mode` is set, the cases
/// header is used and `captured` cases are rendered; otherwise the schema-only
/// header is used and an empty `cases: []` is emitted.
fn render_spec(
    spec_name: &str,
    binding_file: &str,
    registry: &Registry,
    comp: &str,
    depends_on: &[String],
    captured: &[CaseData],
    cases_mode: bool,
) -> String {
    let type_names = registry.type_names();
    let mut s = String::new();
    s.push_str(if cases_mode { CASES_HEADER } else { SPEC_HEADER });
    s.push_str("spec_version: \"0.4.0\"\n");
    let _ = writeln!(s, "name: {spec_name}");
    let _ = writeln!(s, "binding: {binding_file}");

    if !depends_on.is_empty() {
        s.push_str("\ndepends_on:\n");
        for d in depends_on {
            let _ = writeln!(s, "  - {d}");
        }
    }

    let local = registry.local_types(comp);
    if !local.is_empty() {
        s.push_str("\ntypes:\n");
        for t in &local {
            render_type(&mut s, t, &type_names);
        }
    }

    s.push_str("\noperations:\n");
    for op in registry.operations_for(comp) {
        render_operation(&mut s, op, registry, &type_names);
    }

    if cases_mode && !captured.is_empty() {
        render_cases(&mut s, captured);
    } else {
        s.push_str("\ncases: []\n");
    }
    s
}

/// Render captured test cases: one entry per case. Construction inputs (from
/// `$setup.*` echoes) are placed in a case-level `inputs:` block; call inputs
/// go either in the same block (single invocation) or per-step (multi-step).
/// Construction keys come first, then call keys. No `setup:` block anywhere.
fn render_cases(s: &mut String, cases: &[CaseData]) {
    s.push_str("\ncases:\n");
    for c in cases {
        let _ = writeln!(s, "  - name: {}", c.name);
        if let [inv] = c.invocations.as_slice() {
            // Single invocation: merge construction + call inputs at case level.
            // If setup keys exist, emit the merged `inputs:` block BEFORE
            // `operation:` (construction context precedes the call). If there
            // are only call inputs, emit `operation:` first then `inputs:`.
            if c.setup.is_empty() {
                let _ = writeln!(s, "    operation: {}", inv.operation);
                if !inv.inputs.is_empty() {
                    s.push_str("    inputs:\n");
                    for (k, v) in &inv.inputs {
                        let _ = writeln!(s, "      {k}: {v}");
                    }
                }
            } else {
                s.push_str("    inputs:\n");
                for (k, v) in &c.setup {
                    let _ = writeln!(s, "      {k}: {v}");
                }
                for (k, v) in &inv.inputs {
                    let _ = writeln!(s, "      {k}: {v}");
                }
                let _ = writeln!(s, "    operation: {}", inv.operation);
            }
        } else {
            // Multi-step: construction inputs at case level, per-step call inputs at step level.
            if !c.setup.is_empty() {
                s.push_str("    inputs:\n");
                for (k, v) in &c.setup {
                    let _ = writeln!(s, "      {k}: {v}");
                }
            }
            s.push_str("    steps:\n");
            for inv in &c.invocations {
                let _ = writeln!(s, "      - operation: {}", inv.operation);
                if !inv.inputs.is_empty() {
                    s.push_str("        inputs:\n");
                    for (k, v) in &inv.inputs {
                        let _ = writeln!(s, "          {k}: {v}");
                    }
                }
            }
        }
        s.push_str("    expected:\n");
        for ev in &c.expected {
            match ev {
                TraceEvent::Run { operation } => {
                    let _ = writeln!(s, "      - $run: {operation}");
                }
                TraceEvent::Event { name, value } => {
                    let _ = writeln!(s, "      - {name}: {}", render_expected_value(value));
                }
            }
        }
    }
}

/// Render a captured event's value as inline flow YAML (JSON is a valid YAML
/// subset). Scalars such as strings stay quoted (`"5"`), matching the trace the
/// operation self-emits.
fn render_expected_value(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// Render a `map`/`set` type ref as an inline object body (the `type:`/`keys:`/
/// `values:`/`items:` lines) at the given indentation. Scalars are a no-op here
/// — callers render those on the same line as the field/key name.
fn write_collection_body(s: &mut String, ty: &SpecType, child_indent: &str) {
    match ty {
        SpecType::Map { keys, values } => {
            let _ = writeln!(s, "{child_indent}type: map");
            let _ = writeln!(s, "{child_indent}keys: {}", quote_ref(&keys.ref_string()));
            let _ = writeln!(s, "{child_indent}values: {}", quote_ref(&values.ref_string()));
        }
        SpecType::Set { items } => {
            let _ = writeln!(s, "{child_indent}type: set");
            let _ = writeln!(s, "{child_indent}items: {}", quote_ref(&items.ref_string()));
        }
        SpecType::Scalar(_) => {}
    }
}

/// Render a named field (`name: ref`). Scalars (incl. `List<…>` / `Option<…>` /
/// `Result<…>` string shorthand) go inline on one line; `map`/`set` always
/// render as an inline object so every context stays consistent.
fn render_field(s: &mut String, name_indent: &str, child_indent: &str, name: &str, ty: &SpecType) {
    if let SpecType::Scalar(v) = ty {
        let _ = writeln!(s, "{name_indent}{name}: {}", quote_ref(v));
    } else {
        let _ = writeln!(s, "{name_indent}{name}:");
        write_collection_body(s, ty, child_indent);
    }
}

fn render_type(s: &mut String, t: &TypeInfo, type_names: &[&str]) {
    let _ = writeln!(s, "  {}:", t.name);
    if t.kind == "enum" {
        s.push_str("    oneof:\n");
        for v in &t.variants {
            if v.fields.is_empty() {
                let _ = writeln!(s, "      {}: {{}}", v.name);
            } else {
                let _ = writeln!(s, "      {}:", v.name);
                for (fname, fty) in &v.fields {
                    render_field(s, "        ", "          ", fname, &map_type(fty, type_names));
                }
            }
        }
    } else {
        for (fname, fty) in &t.fields {
            render_field(s, "    ", "      ", fname, &map_type(fty, type_names));
        }
    }
}

fn render_operation(s: &mut String, op: &OpInfo, registry: &Registry, type_names: &[&str]) {
    let _ = writeln!(s, "  {}:", op.name);
    if op.is_async {
        s.push_str("    async: true\n");
    }
    let inputs = build_inputs(op, registry);
    if !inputs.is_empty() {
        s.push_str("    inputs:\n");
        for (name, ty) in &inputs {
            // Inputs are emitted plain (a `key: value` line); scalar refs such
            // as `List<i32>` are valid unquoted plain YAML scalars. `map`/`set`
            // always render as inline objects.
            if let SpecType::Scalar(v) = ty {
                let _ = writeln!(s, "      {name}: {v}");
            } else {
                let _ = writeln!(s, "      {name}:");
                write_collection_body(s, ty, "        ");
            }
        }
    }
    // Unit return ⇒ no `$result` output.
    let ret = map_type(&op.return_type, type_names);
    if !is_unit(&op.return_type) {
        s.push_str("    outputs:\n");
        if let SpecType::Scalar(v) = &ret {
            let _ = writeln!(s, "      - $result: {}", quote_ref(v));
        } else {
            s.push_str("      - $result:\n");
            write_collection_body(s, &ret, "          ");
        }
    }
}

fn render_binding(rel_pkg: &str) -> String {
    let mut s = String::new();
    s.push_str(BINDING_HEADER);
    s.push_str("language: rust\n");
    s.push_str("targets:\n");
    s.push_str("  default:\n");
    let _ = writeln!(s, "    package_root: {rel_pkg}");
    s
}

// ---------------------------------------------------------------------------
// Terminal formatting
// ---------------------------------------------------------------------------

/// Render an extract outcome to a colored, human-readable string for the
/// terminal (used by the binary).
#[must_use]
pub fn format_outcome(outcome: &ExtractOutcome) -> String {
    let mut s = String::new();
    match outcome {
        ExtractOutcome::Error { reason } => {
            let _ = writeln!(s, "\x1b[31merror:\x1b[0m {reason}");
        }
        ExtractOutcome::Complete { report } => {
            let _ = writeln!(s, "spec: {}", report.spec_name);
            let _ = writeln!(
                s,
                "\x1b[32mextracted:\x1b[0m {} operation(s), {} type(s) -> {}",
                report.operations, report.types, report.output_path
            );
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use specgate_harness::discovery::{VariantInfo, is_builtin_type, is_runtime_value};

    fn names() -> Vec<&'static str> {
        vec!["Shape", "Balance", "Money"]
    }

    #[test]
    fn maps_scalars() {
        let n = names();
        assert_eq!(map_type("i32", &n), SpecType::Scalar("i32".into()));
        assert_eq!(map_type("i64", &n), SpecType::Scalar("i64".into()));
        assert_eq!(map_type("f64", &n), SpecType::Scalar("f64".into()));
        assert_eq!(map_type("bool", &n), SpecType::Scalar("bool".into()));
        assert_eq!(map_type("String", &n), SpecType::Scalar("string".into()));
        assert_eq!(map_type("& str", &n), SpecType::Scalar("string".into()));
    }

    #[test]
    fn maps_named_spec_event_type() {
        assert_eq!(map_type("Shape", &names()), SpecType::Scalar("Shape".into()));
        assert_eq!(map_type("Balance", &names()), SpecType::Scalar("Balance".into()));
    }

    #[test]
    fn maps_runtime_value_to_builtin_value() {
        let n = names();
        // The runtime `Value` maps to the built-in `value` in all stringified forms.
        assert_eq!(map_type("Value", &n), SpecType::Scalar("value".into()));
        assert_eq!(map_type("specgate_runtime :: Value", &n), SpecType::Scalar("value".into()));
        assert_eq!(map_type(":: specgate_runtime :: Value", &n), SpecType::Scalar("value".into()));
        assert_eq!(map_type("specgate :: Value", &n), SpecType::Scalar("value".into()));
        // Recurses through collections and Option.
        assert_eq!(map_type("Vec < Value >", &n).ref_string(), "List<value>");
        assert_eq!(map_type("& [ Value ]", &n).ref_string(), "List<value>");
        assert_eq!(map_type("Option < Value >", &n).ref_string(), "Option<value>");
        assert_eq!(
            map_type("BTreeMap < String, Value >", &n),
            SpecType::Map {
                keys: Box::new(SpecType::Scalar("string".into())),
                values: Box::new(SpecType::Scalar("value".into())),
            }
        );
        assert_eq!(
            map_type("HashSet < Value >", &n),
            SpecType::Set {
                items: Box::new(SpecType::Scalar("value".into())),
            }
        );
    }

    #[test]
    fn runtime_value_is_builtin_not_a_named_ref() {
        // `Value` is a built-in, so it is never collected as a dependency ref.
        let mut refs = Vec::new();
        collect_named_refs("Vec < Value >", &mut refs);
        assert!(refs.is_empty(), "unexpected refs: {refs:?}");
        assert!(is_builtin_type("Value"));
        assert!(is_runtime_value("Value"));
        assert!(!is_runtime_value("Values"));
    }

    #[test]
    fn maps_option_list_result_shorthands() {
        let n = names();
        assert_eq!(map_type("Option < i32 >", &n).ref_string(), "Option<i32>");
        assert_eq!(map_type("Vec < i32 >", &n).ref_string(), "List<i32>");
        assert_eq!(map_type("& [ i32 ]", &n).ref_string(), "List<i32>");
        assert_eq!(map_type("Result < i32, String >", &n).ref_string(), "Result<i32, string>");
    }

    #[test]
    fn maps_map_and_set_to_inline_objects() {
        let n = names();
        assert_eq!(
            map_type("BTreeMap < String, i32 >", &n),
            SpecType::Map {
                keys: Box::new(SpecType::Scalar("string".into())),
                values: Box::new(SpecType::Scalar("i32".into())),
            }
        );
        assert_eq!(
            map_type("BTreeSet < String >", &n),
            SpecType::Set {
                items: Box::new(SpecType::Scalar("string".into())),
            }
        );
        assert_eq!(
            map_type("std :: collections :: HashMap < String, i32 >", &n),
            SpecType::Map {
                keys: Box::new(SpecType::Scalar("string".into())),
                values: Box::new(SpecType::Scalar("i32".into())),
            }
        );
    }

    #[test]
    fn maps_nested_generics() {
        let n = names();
        assert_eq!(map_type("Vec < Option < i32 > >", &n).ref_string(), "List<Option<i32>>");
    }

    #[test]
    fn binding_name_from_out() {
        assert_eq!(binding_file_name(Path::new("a/b/extracted.spec.yaml")), "extracted.binding.yaml");
        assert_eq!(binding_file_name(Path::new("foo.yaml")), "foo.binding.yaml");
    }

    #[test]
    fn is_unit_detects_void() {
        assert!(is_unit("()"));
        assert!(is_unit(""));
        assert!(!is_unit("i32"));
    }

    #[test]
    fn quote_ref_quotes_generics_only() {
        assert_eq!(quote_ref("i32"), "i32");
        assert_eq!(quote_ref("Shape"), "Shape");
        assert_eq!(quote_ref("Option<i32>"), "\"Option<i32>\"");
        assert_eq!(quote_ref("Result<i32, string>"), "\"Result<i32, string>\"");
    }

    // --- setup-fill logic -------------------------------------------------

    fn op(name: &str, is_setup: bool, return_type: &str, fills: &str, params: &[(&str, &str)]) -> OpInfo {
        OpInfo {
            name: name.into(),
            is_setup,
            is_async: false,
            return_type: return_type.into(),
            fills: fills.into(),
            params: params.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect(),
            component: String::new(),
        }
    }

    #[test]
    fn setup_fills_param_by_type_omits_input() {
        // double(x: i32) with setup seed() -> i32 fills x; seed has no params.
        let reg = Registry {
            ops: vec![op("double", false, "i32", "", &[("x", "i32")]), op("double", true, "i32", "", &[])],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        assert!(inputs.is_empty(), "x is setup-filled and omitted: {inputs:?}");
    }

    #[test]
    fn receiver_setup_injects_params_at_front() {
        // scale(value: i32) method; setup make_scaler(factor: i32) -> Scaler
        // fills the receiver, injecting `factor` before the kept `value`.
        let reg = Registry {
            ops: vec![
                op("scale", false, "i32", "", &[("value", "i32")]),
                op("scale", true, "Scaler", "", &[("factor", "i32")]),
            ],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["factor", "value"]);
    }

    #[test]
    fn no_setup_keeps_params_in_order() {
        let reg = Registry {
            ops: vec![op("add", false, "i32", "", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a", "b"]);
    }

    #[test]
    fn fills_pins_specific_param() {
        // Two i32 params; a setup with fills="b" must target b, not a.
        let reg = Registry {
            ops: vec![
                op("f", false, "i32", "", &[("a", "i32"), ("b", "i32")]),
                op("f", true, "i32", "b", &[]),
            ],
            types: vec![],
        };
        let inputs = build_inputs(&reg.ops[0], &reg);
        let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a"], "only b is filled/omitted");
    }

    #[test]
    fn registry_parses_and_filters_setups() {
        let json = r#"{"operations":[
            {"name":"add","is_setup":false,"is_async":false,"return_type":"i32","fills":"","params":[["a","i32"]]},
            {"name":"double","is_setup":true,"is_async":false,"return_type":"i32","fills":"","params":[]}
        ],"types":[
            {"name":"Money","module_path":"m","kind":"struct","fields":[["cents","i64"]],"variants":[]}
        ]}"#;
        let reg = Registry::parse(json).expect("parse");
        assert_eq!(reg.operations_for("").len(), 1);
        assert_eq!(reg.types.len(), 1);
        assert_eq!(reg.setups_for("double").len(), 1);
    }

    #[test]
    fn renders_struct_and_enum_types() {
        let reg = Registry {
            ops: vec![],
            types: vec![
                TypeInfo {
                    name: "Money".into(),
                    kind: "struct".into(),
                    fields: vec![("cents".into(), "i64".into())],
                    variants: vec![],
                    component: String::new(),
                },
                TypeInfo {
                    name: "Shape".into(),
                    kind: "enum".into(),
                    fields: vec![],
                    variants: vec![
                        VariantInfo {
                            name: "Circle".into(),
                            fields: vec![("radius".into(), "f64".into())],
                        },
                        VariantInfo {
                            name: "Point".into(),
                            fields: vec![],
                        },
                    ],
                    component: String::new(),
                },
            ],
        };
        let yaml = render_spec("demo", "demo.binding.yaml", &reg, "", &[], &[], false);
        assert!(yaml.contains("  Money:\n    cents: i64\n"), "{yaml}");
        assert!(
            yaml.contains("  Shape:\n    oneof:\n      Circle:\n        radius: f64\n      Point: {}\n"),
            "{yaml}"
        );
        assert!(yaml.ends_with("\ncases: []\n"));
    }

    #[test]
    fn renders_binding() {
        let b = render_binding("..");
        assert!(b.contains("language: rust\n"));
        assert!(b.contains("    package_root: ..\n"));
    }

    #[test]
    fn error_when_package_root_missing() {
        let out = extract("definitely/does/not/exist", "x.spec.yaml", "", false);
        match out {
            ExtractOutcome::Error { reason } => assert!(reason.contains("package_root not found"), "{reason}"),
            ExtractOutcome::Complete { report } => panic!("expected error, got {report:?}"),
        }
    }

    #[test]
    fn error_when_not_a_crate() {
        // `src` (this crate's source dir, the test CWD's child) exists but has
        // no Cargo.toml — extraction must reject it before writing anything.
        let out = extract("src", "x.spec.yaml", "", false);
        match out {
            ExtractOutcome::Error { reason } => assert!(reason.contains("no Cargo.toml"), "{reason}"),
            ExtractOutcome::Complete { report } => panic!("expected error, got {report:?}"),
        }
    }

    #[test]
    fn display_formats_outcomes() {
        let c = ExtractOutcome::Complete {
            report: ExtractReport {
                spec_name: "x".into(),
                operations: 2,
                types: 1,
                cases: 0,
                output_path: "o".into(),
            },
        };
        assert!(format!("{c}").contains("Complete(spec=x"));
        assert!(format_outcome(&c).contains("extracted:"));
        let e = ExtractOutcome::Error { reason: "boom".into() };
        assert_eq!(format!("{e}"), "Error(boom)");
        assert!(format_outcome(&e).contains("boom"));
    }

    // --- component axis ---------------------------------------------------

    fn ty(name: &str, comp: &str, fields: &[(&str, &str)]) -> TypeInfo {
        TypeInfo {
            name: name.into(),
            kind: "struct".into(),
            fields: fields.iter().map(|(a, b)| ((*a).to_string(), (*b).to_string())).collect(),
            variants: vec![],
            component: comp.into(),
        }
    }

    fn op_c(name: &str, comp: &str, return_type: &str, params: &[(&str, &str)]) -> OpInfo {
        let mut o = op(name, false, return_type, "", params);
        o.component = comp.into();
        o
    }

    #[test]
    fn present_components_sorted_distinct() {
        let reg = Registry {
            ops: vec![
                op_c("b_op", "comp.b", "i32", &[]),
                op_c("a_op", "comp.a", "i32", &[]),
                op_c("a_op2", "comp.a", "i32", &[]),
            ],
            types: vec![ty("T", "comp.b", &[]), ty("U", "comp.core", &[])],
        };
        assert_eq!(reg.present_components(), vec!["comp.a", "comp.b", "comp.core"]);
    }

    #[test]
    fn operations_and_local_types_filter_by_component() {
        let reg = Registry {
            ops: vec![op_c("x", "comp.a", "i32", &[]), op_c("y", "comp.b", "i32", &[])],
            types: vec![ty("Ta", "comp.a", &[]), ty("Tb", "comp.b", &[])],
        };
        let ops_a: Vec<&str> = reg.operations_for("comp.a").iter().map(|o| o.name.as_str()).collect();
        assert_eq!(ops_a, vec!["x"]);
        let types_a: Vec<&str> = reg.local_types("comp.a").iter().map(|t| t.name.as_str()).collect();
        assert_eq!(types_a, vec!["Ta"]);
    }

    #[test]
    fn resolve_dependencies_local_only_is_empty() {
        let reg = Registry {
            ops: vec![op_c("make", "comp.core", "Widget", &[])],
            types: vec![ty("Widget", "comp.core", &[("id", "i32")])],
        };
        assert_eq!(resolve_dependencies(&reg, "comp.core").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn resolve_dependencies_foreign_ref_adds_component() {
        let reg = Registry {
            ops: vec![
                op_c("assemble", "comp.app", "Widget", &[]),
                op_c("make", "comp.core", "Widget", &[]),
            ],
            types: vec![ty("Widget", "comp.core", &[("id", "i32")])],
        };
        assert_eq!(resolve_dependencies(&reg, "comp.app").unwrap(), vec!["comp.core".to_string()]);
    }

    #[test]
    fn resolve_dependencies_unresolved_type_errors() {
        let reg = Registry {
            ops: vec![op_c("assemble", "comp.app", "Gadget", &[])],
            types: vec![],
        };
        let err = resolve_dependencies(&reg, "comp.app").unwrap_err();
        assert!(err.contains("unresolved type"), "{err}");
        assert!(err.contains("Gadget"), "{err}");
    }

    #[test]
    fn collect_named_refs_collects_named_only() {
        let mut a = Vec::new();
        collect_named_refs("Vec < Widget >", &mut a);
        assert_eq!(a, vec!["Widget".to_string()]);

        let mut b = Vec::new();
        collect_named_refs("Result < i32, String >", &mut b);
        assert!(b.is_empty(), "{b:?}");

        let mut c = Vec::new();
        collect_named_refs("Option < Widget >", &mut c);
        assert_eq!(c, vec!["Widget".to_string()]);
    }

    // --- Case capture ---------------------------------------------------------

    fn run(op: &str) -> TraceEvent {
        TraceEvent::Run { operation: op.into() }
    }

    fn ev(name: &str, value: &str) -> TraceEvent {
        TraceEvent::Event {
            name: name.into(),
            value: Value::String(value.into()),
        }
    }

    #[test]
    fn segments_single_invocation_and_recovers_inputs() {
        let trace = vec![run("add"), ev("add.a", "2"), ev("add.b", "3"), ev("$result", "5")];
        let invs = segment_invocations(&trace);
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].operation, "add");
        assert_eq!(
            invs[0].inputs,
            vec![("a".to_string(), "2".to_string()), ("b".to_string(), "3".to_string())]
        );
    }

    #[test]
    fn segments_multiple_invocations_by_run_events() {
        let trace = vec![
            run("add"),
            ev("add.a", "2"),
            ev("add.b", "3"),
            ev("$result", "5"),
            run("add"),
            ev("add.a", "10"),
            ev("add.b", "20"),
            ev("$result", "30"),
        ];
        let invs = segment_invocations(&trace);
        assert_eq!(invs.len(), 2);
        assert_eq!(
            invs[1].inputs,
            vec![("a".to_string(), "10".to_string()), ("b".to_string(), "20".to_string())]
        );
    }

    #[test]
    fn segmentation_excludes_result_and_field_events_from_inputs() {
        // `$result` and bare field events (no `<op>.` prefix) are not inputs.
        let trace = vec![run("greet"), ev("greet.name", "world"), ev("$result", "hello, world")];
        let invs = segment_invocations(&trace);
        assert_eq!(invs[0].inputs, vec![("name".to_string(), "world".to_string())]);
    }

    #[test]
    fn free_fn_invocation_detected_when_all_params_echoed() {
        let reg = Registry {
            ops: vec![op_c("add", "c", "i32", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let inv = CaseInvocation {
            operation: "add".into(),
            inputs: vec![("a".into(), "2".into()), ("b".into(), "3".into())],
        };
        assert!(is_free_fn_invocation(&inv, &reg, "c"));
    }

    #[test]
    fn method_invocation_rejected_when_a_param_has_no_echo() {
        // `withdraw(amount)` with no `withdraw.amount` echo → rejected
        // (param is declared but not echoed, so inputs are not fully recoverable).
        let reg = Registry {
            ops: vec![op_c("withdraw", "c", "()", &[("amount", "i32")])],
            types: vec![],
        };
        let inv_missing = CaseInvocation {
            operation: "withdraw".into(),
            inputs: vec![],
        };
        assert!(!is_free_fn_invocation(&inv_missing, &reg, "c"));

        // With the echo present the invocation IS accepted (method params now echo).
        let inv_echoed = CaseInvocation {
            operation: "withdraw".into(),
            inputs: vec![("amount".into(), "50".into())],
        };
        assert!(is_free_fn_invocation(&inv_echoed, &reg, "c"));
    }

    #[test]
    fn sanitize_case_name_normalizes_and_prefixes() {
        assert_eq!(sanitize_case_name("adds_two_and_three"), "adds_two_and_three");
        assert_eq!(sanitize_case_name("Adds-Two"), "adds_two");
        assert_eq!(sanitize_case_name("tests::adds"), "tests__adds");
        assert_eq!(sanitize_case_name("1abc"), "c1abc");
        assert_eq!(sanitize_case_name("_hidden"), "c_hidden");
    }

    #[test]
    fn last_segment_takes_final_module_component() {
        assert_eq!(last_segment("tests::adds_several"), "adds_several");
        assert_eq!(last_segment("adds_several"), "adds_several");
        assert_eq!(last_segment("a::b::c"), "c");
    }

    #[test]
    fn build_cases_orders_alphabetically_and_shapes_steps() {
        let reg = Registry {
            ops: vec![op_c("add", "c", "i32", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let raw = vec![
            (
                "tests::adds_several".to_string(),
                vec![
                    run("add"),
                    ev("add.a", "2"),
                    ev("add.b", "3"),
                    ev("$result", "5"),
                    run("add"),
                    ev("add.a", "10"),
                    ev("add.b", "20"),
                    ev("$result", "30"),
                ],
            ),
            (
                "tests::adds_two_and_three".to_string(),
                vec![run("add"), ev("add.a", "2"), ev("add.b", "3"), ev("$result", "5")],
            ),
        ];
        let cases = build_cases(raw, &reg, "c");
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["adds_several", "adds_two_and_three"]);
        // Multi-invocation -> steps; single -> simple.
        assert_eq!(cases[0].invocations.len(), 2);
        assert_eq!(cases[1].invocations.len(), 1);
    }

    #[test]
    fn build_cases_skips_empty_and_method_tests() {
        let reg = Registry {
            ops: vec![
                op_c("add", "c", "i32", &[("a", "i32"), ("b", "i32")]),
                op_c("withdraw", "c", "()", &[("amount", "i32")]),
            ],
            types: vec![],
        };
        let raw = vec![
            // Empty trace — no operations exercised.
            ("tests::no_ops".to_string(), vec![]),
            // Method invocation with no param echo → still rejected.
            ("tests::uses_method_missing_echo".to_string(), vec![run("withdraw")]),
            // Method invocation WITH param echo → now accepted.
            (
                "tests::uses_method_echoed".to_string(),
                vec![run("withdraw"), ev("withdraw.amount", "50")],
            ),
            (
                "tests::good".to_string(),
                vec![run("add"), ev("add.a", "1"), ev("add.b", "2"), ev("$result", "3")],
            ),
        ];
        let cases = build_cases(raw, &reg, "c");
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["good", "uses_method_echoed"]);
    }

    #[test]
    fn build_cases_qualifies_colliding_bare_names() {
        let reg = Registry {
            ops: vec![op_c("add", "c", "i32", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let raw = vec![
            (
                "mod_a::adds".to_string(),
                vec![run("add"), ev("add.a", "1"), ev("add.b", "1"), ev("$result", "2")],
            ),
            (
                "mod_b::adds".to_string(),
                vec![run("add"), ev("add.a", "2"), ev("add.b", "2"), ev("$result", "4")],
            ),
        ];
        let cases = build_cases(raw, &reg, "c");
        let names: Vec<&str> = cases.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["mod_a_adds", "mod_b_adds"]);
    }

    #[test]
    fn render_expected_value_quotes_strings() {
        assert_eq!(render_expected_value(&Value::String("5".into())), "\"5\"");
        assert_eq!(render_expected_value(&Value::String("hello, world".into())), "\"hello, world\"");
    }

    #[test]
    fn render_cases_uses_cases_header_and_matches_shape() {
        let reg = Registry {
            ops: vec![op_c("add", "fixture.cases", "i32", &[("a", "i32"), ("b", "i32")])],
            types: vec![],
        };
        let cases = vec![CaseData {
            name: "adds_two_and_three".into(),
            setup: BTreeMap::new(),
            invocations: vec![CaseInvocation {
                operation: "add".into(),
                inputs: vec![("a".into(), "2".into()), ("b".into(), "3".into())],
            }],
            expected: vec![run("add"), ev("add.a", "2"), ev("add.b", "3"), ev("$result", "5")],
        }];
        let yaml = render_spec(
            "fixture.cases",
            "fixture.cases.binding.yaml",
            &reg,
            "fixture.cases",
            &[],
            &cases,
            true,
        );
        assert!(
            yaml.starts_with("# Spec extracted from your code by `specgate extract --cases`."),
            "{yaml}"
        );
        assert!(
            yaml.contains("cases:\n  - name: adds_two_and_three\n    operation: add\n    inputs:\n      a: 2\n      b: 3\n"),
            "{yaml}"
        );
        assert!(
            yaml.contains("    expected:\n      - $run: add\n      - add.a: \"2\"\n      - add.b: \"3\"\n      - $result: \"5\"\n"),
            "{yaml}"
        );
        assert!(!yaml.contains("\ncases: []\n"), "{yaml}");
    }

    // --- setup extraction and filtering -----------------------------------

    fn ev_int(name: &str, i: i64) -> TraceEvent {
        TraceEvent::Event {
            name: name.into(),
            value: Value::Integer(i),
        }
    }

    #[test]
    fn segment_trace_extracts_setup_events() {
        let trace = vec![
            TraceEvent::Event {
                name: "$setup.start".into(),
                value: Value::String("10".into()),
            },
            run("adjust"),
            ev("adjust.amount", "5"),
            ev("$result", "15"),
        ];
        let (setup, invs) = segment_trace(&trace);
        assert_eq!(setup.get("start").map(String::as_str), Some("10"));
        assert_eq!(invs.len(), 1);
        assert_eq!(invs[0].inputs, vec![("amount".to_string(), "5".to_string())]);
    }

    #[test]
    fn filter_expected_keeps_run_result_and_op_echoes() {
        let reg = Registry {
            ops: vec![op_c("adjust", "c", "i32", &[("amount", "i32")])],
            types: vec![],
        };
        let trace = vec![
            TraceEvent::Event {
                name: "$setup.start".into(),
                value: Value::String("10".into()),
            },
            run("adjust"),
            ev("adjust.amount", "5"),
            ev_int("total", 15),
            ev("$result", "15"),
        ];
        let filtered = filter_expected(&trace, &reg, "c");
        assert_eq!(filtered.len(), 3, "setup and bare field events must be excluded");
        assert!(matches!(&filtered[0], TraceEvent::Run { operation } if operation == "adjust"));
        assert!(matches!(&filtered[1], TraceEvent::Event { name, .. } if name == "adjust.amount"));
        assert!(matches!(&filtered[2], TraceEvent::Event { name, .. } if name == "$result"));
    }

    #[test]
    fn render_cases_with_setup_map() {
        let cases = vec![CaseData {
            name: "adjusts_from_ten".into(),
            setup: {
                let mut m = BTreeMap::new();
                m.insert("start".to_string(), "10".to_string());
                m
            },
            invocations: vec![CaseInvocation {
                operation: "adjust".into(),
                inputs: vec![("amount".into(), "5".into())],
            }],
            expected: vec![run("adjust"), ev("adjust.amount", "5"), ev("$result", "15")],
        }];
        let mut s = String::new();
        render_cases(&mut s, &cases);
        // Construction inputs merged with call inputs into one case-level inputs: block
        assert!(s.contains("    inputs:\n      start: 10\n      amount: 5\n"), "{s}");
        assert!(s.contains("    operation: adjust\n"), "{s}");
        assert!(!s.contains("    setup:"), "no setup: block should appear: {s}");
    }

    #[test]
    fn build_cases_with_setup_backed_method() {
        let reg = Registry {
            ops: vec![
                op_c("adjust", "c", "i32", &[("amount", "i32")]),
                // setup for adjust (is_setup = true)
                {
                    let mut o = op("adjust", true, "Tally", "", &[("start", "i32")]);
                    o.component = "c".into();
                    o
                },
            ],
            types: vec![],
        };
        let raw = vec![(
            "tests::adjusts_from_ten".to_string(),
            vec![
                TraceEvent::Event {
                    name: "$setup.start".into(),
                    value: Value::String("10".into()),
                },
                run("adjust"),
                ev("adjust.amount", "5"),
                ev_int("total", 15),
                ev("$result", "15"),
            ],
        )];
        let cases = build_cases(raw, &reg, "c");
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "adjusts_from_ten");
        assert_eq!(cases[0].setup.get("start").map(String::as_str), Some("10"));
        assert_eq!(cases[0].invocations.len(), 1);
        assert_eq!(cases[0].invocations[0].inputs, vec![("amount".to_string(), "5".to_string())]);
        // expected must not contain $setup.start or total
        let exp_names: Vec<&str> = cases[0]
            .expected
            .iter()
            .map(|e| match e {
                TraceEvent::Run { operation } => operation.as_str(),
                TraceEvent::Event { name, .. } => name.as_str(),
            })
            .collect();
        assert!(!exp_names.contains(&"$setup.start"), "setup event must not be in expected");
        assert!(!exp_names.contains(&"total"), "bare field event must not be in expected");
        assert!(exp_names.contains(&"adjust.amount"), "param echo must be in expected");
    }
}
