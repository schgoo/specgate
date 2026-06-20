//! `specgate validate <spec-dir>` — static checks across one or more
//! `.spec.yaml` files in a directory tree.

use regex::Regex;
use serde_yaml::Value;
use specgate_annotations::spec_operation;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationFinding {
    pub severity: Severity,
    pub check: String,
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationReport {
    pub total_files: i32,
    pub errors: i32,
    pub warnings: i32,
    pub findings: Vec<ValidationFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidateOutcome {
    Pass { report: ValidationReport },
    Fail { report: ValidationReport },
}

impl std::fmt::Display for ValidateOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidateOutcome::Pass { report } => write!(
                f,
                "Pass(files={}, errors={}, warnings={})",
                report.total_files, report.errors, report.warnings
            ),
            ValidateOutcome::Fail { report } => write!(
                f,
                "Fail(files={}, errors={}, warnings={})",
                report.total_files, report.errors, report.warnings
            ),
        }
    }
}

#[spec_operation("validate")]
pub fn validate(spec_dir: &str, strict: bool, suppress: &[String], assertions_dir: &str, check_source: bool) -> ValidateOutcome {
    let suppress_set: HashSet<String> = suppress.iter().cloned().collect();
    let mut findings: Vec<ValidationFinding> = Vec::new();
    let mut total_files = 0;

    // 2. Resolve the assertions directory. An explicit dir is used as-is;
    // otherwise default to `<spec_dir>/sources/assertions`. Assertion-aware
    // checks only run when the resolved directory actually exists.
    let resolved_assertions_dir: PathBuf = if assertions_dir.is_empty() {
        Path::new(spec_dir).join("sources").join("assertions")
    } else {
        PathBuf::from(assertions_dir)
    };
    let assertions_active = resolved_assertions_dir.exists();
    let assertions: BTreeMap<String, Assertion> = if assertions_active {
        load_assertions(&resolved_assertions_dir)
    } else {
        BTreeMap::new()
    };

    let files = collect_spec_files(Path::new(spec_dir));
    for path in files {
        total_files += 1;
        let file_str = path.to_string_lossy().to_string();
        let Ok(raw) = std::fs::read_to_string(&path) else { continue };
        let Ok(value) = serde_yaml::from_str::<Value>(&raw) else {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                check: "schema".into(),
                file: file_str.clone(),
                message: "spec file is not valid YAML".into(),
            });
            continue;
        };
        check_file(
            &value,
            &file_str,
            &path,
            &assertions,
            assertions_active,
            check_source,
            &mut findings,
        );
    }

    findings.retain(|f| !suppress_set.contains(&f.check));

    if strict {
        for f in &mut findings {
            if f.severity == Severity::Warn {
                f.severity = Severity::Error;
            }
        }
    }

    let mut errors = 0;
    let mut warnings = 0;
    for f in &findings {
        match f.severity {
            Severity::Error => errors += 1,
            Severity::Warn => warnings += 1,
            Severity::Info => {}
        }
    }

    let report = ValidationReport {
        total_files,
        errors,
        warnings,
        findings,
    };
    if report.errors == 0 {
        ValidateOutcome::Pass { report }
    } else {
        ValidateOutcome::Fail { report }
    }
}

#[derive(Debug, Clone)]
struct Assertion {
    level: String,
    negatable: bool,
}

/// Normalize an RFC 2119 keyword into one of "must", "should", "may", or the
/// lowercased input for anything unrecognized.
fn normalize_level(raw: &str) -> String {
    match raw.trim().to_uppercase().as_str() {
        "MUST" | "REQUIRED" | "SHALL" => "must".to_string(),
        "SHOULD" | "RECOMMENDED" => "should".to_string(),
        "MAY" | "OPTIONAL" => "may".to_string(),
        other => other.to_lowercase(),
    }
}

/// Recursively load assertion source files (`.yaml`/`.yml`) from `dir`, keyed
/// by their `id` field. Each file is a mapping with `id`, `level`, and an
/// optional `negatable` flag.
fn load_assertions(dir: &Path) -> BTreeMap<String, Assertion> {
    let mut map = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            let Ok(raw) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(val) = serde_yaml::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(m) = val.as_mapping() else { continue };
            let Some(id) = m.get(Value::String("id".into())).and_then(|v| v.as_str()) else {
                continue;
            };
            let level = m.get(Value::String("level".into())).and_then(|v| v.as_str()).unwrap_or("");
            let negatable = m.get(Value::String("negatable".into())).and_then(Value::as_bool).unwrap_or(false);
            map.insert(
                id.to_string(),
                Assertion {
                    level: normalize_level(level),
                    negatable,
                },
            );
        }
    }
    map
}

fn collect_spec_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk(&p, out);
        } else if let Some(name) = p.file_name().and_then(|s| s.to_str())
            && name.ends_with(".spec.yaml")
        {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// Per-file checks
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct OpDecl {
    declared_inputs: Vec<String>,
}

fn check_file(
    spec: &Value,
    file: &str,
    path: &Path,
    assertions: &BTreeMap<String, Assertion>,
    assertions_active: bool,
    check_source: bool,
    findings: &mut Vec<ValidationFinding>,
) {
    let _ = path;
    let Some(map) = spec.as_mapping() else {
        findings.push(ValidationFinding {
            severity: Severity::Error,
            check: "schema".into(),
            file: file.into(),
            message: "spec top-level is not a mapping".into(),
        });
        return;
    };

    // 1. schema: spec_version is required
    if map.get(Value::String("spec_version".into())).is_none() {
        findings.push(ValidationFinding {
            severity: Severity::Error,
            check: "schema".into(),
            file: file.into(),
            message: "missing required field 'spec_version'".into(),
        });
    }

    // Parse operations
    let mut ops: BTreeMap<String, OpDecl> = BTreeMap::new();
    if let Some(Value::Mapping(ops_map)) = map.get(Value::String("operations".into())) {
        for (k, v) in ops_map {
            let Some(name) = k.as_str() else { continue };
            let mut decl = OpDecl::default();
            if let Some(body) = v.as_mapping()
                && let Some(Value::Mapping(inputs)) = body.get(Value::String("inputs".into()))
            {
                for (ik, _) in inputs {
                    if let Some(s) = ik.as_str() {
                        decl.declared_inputs.push(s.to_string());
                    }
                }
            }
            ops.insert(name.to_string(), decl);
        }
    }

    // dep_consistency: each operation's `depends_on` must reference another
    // declared operation. Iterate operations and their deps in document order.
    if let Some(Value::Mapping(ops_map)) = map.get(Value::String("operations".into())) {
        for (k, v) in ops_map {
            let Some(op_name) = k.as_str() else { continue };
            let Some(body) = v.as_mapping() else { continue };
            if let Some(Value::Sequence(deps)) = body.get(Value::String("depends_on".into())) {
                for dep in deps {
                    if let Some(dep_name) = dep.as_str()
                        && !ops.contains_key(dep_name)
                    {
                        findings.push(ValidationFinding {
                            severity: Severity::Error,
                            check: "dep_consistency".into(),
                            file: file.into(),
                            message: format!("operation '{op_name}' depends on undefined operation '{dep_name}'"),
                        });
                    }
                }
            }
        }
    }

    let cases_v = map.get(Value::String("cases".into()));
    let cases_seq: Vec<&Value> = match cases_v.and_then(|c| c.as_sequence()) {
        Some(s) => s.iter().collect(),
        None => Vec::new(),
    };

    let mut referenced_ids: BTreeMap<String, bool> = BTreeMap::new();
    let mut seen_names: BTreeSet<String> = BTreeSet::new();

    for c in &cases_seq {
        let Some(cm) = c.as_mapping() else { continue };
        let name = cm
            .get(Value::String("name".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let kind = cm.get(Value::String("kind".into())).and_then(|v| v.as_str()).unwrap_or("");
        let is_narrative = kind == "narrative";
        let level = cm.get(Value::String("level".into())).and_then(|v| v.as_str());
        let operation = cm
            .get(Value::String("operation".into()))
            .and_then(|v| v.as_str())
            .map(ToString::to_string);

        // 3. name_uniqueness
        if !name.is_empty() {
            if seen_names.contains(&name) {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    check: "name_uniqueness".into(),
                    file: file.into(),
                    message: format!("duplicate case name '{name}'"),
                });
            } else {
                seen_names.insert(name.clone());
            }
        }

        // source.assertion_ids
        let source_ids: Vec<String> = cm
            .get(Value::String("source".into()))
            .and_then(|v| v.as_mapping())
            .and_then(|m| m.get(Value::String("assertion_ids".into())))
            .and_then(|v| v.as_sequence())
            .map(|s| s.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default();

        if is_narrative {
            // 7. narrative_misuse: if verify steps look testable, warn.
            let verify = cm.get(Value::String("verify".into())).and_then(|v| v.as_sequence());
            if let Some(verify) = verify {
                let any_testable = verify
                    .iter()
                    .any(|step| if let Some(s) = step.as_str() { looks_testable(s) } else { false });
                if any_testable {
                    findings.push(ValidationFinding {
                        severity: Severity::Warn,
                        check: "narrative_misuse".into(),
                        file: file.into(),
                        message: format!("narrative case '{name}' has verify steps that look testable"),
                    });
                }
            }
            continue;
        }

        // 2. operation_reference
        if let Some(op) = operation.as_deref() {
            if ops.contains_key(op) {
                // operation exists, proceed
                // 5. input_completeness: missing and extra inputs
                let has_setup = cm.get(Value::String("setup".into())).is_some();
                let provided: BTreeSet<String> = cm
                    .get(Value::String("inputs".into()))
                    .and_then(|v| v.as_mapping())
                    .map(|m| m.iter().filter_map(|(k, _)| k.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                let decl = &ops[op];
                let declared_set: BTreeSet<String> = decl.declared_inputs.iter().cloned().collect();

                for required in &decl.declared_inputs {
                    if !provided.contains(required) {
                        findings.push(ValidationFinding {
                            severity: Severity::Error,
                            check: "input_completeness".into(),
                            file: file.into(),
                            message: format!("case '{name}' missing required input '{required}' for operation '{op}'"),
                        });
                    }
                }

                // Skip extra-inputs check when case has setup: (mock injection pattern)
                if !has_setup {
                    for extra in provided.difference(&declared_set) {
                        findings.push(ValidationFinding {
                            severity: Severity::Error,
                            check: "input_completeness".into(),
                            file: file.into(),
                            message: format!("case '{name}' has extra input '{extra}' not declared in operation '{op}'"),
                        });
                    }
                }
            } else {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    check: "operation_reference".into(),
                    file: file.into(),
                    message: format!("case '{name}' references undefined operation '{op}'"),
                });
            }
        }

        // 6. expected_format: each entry must have exactly one key.
        if let Some(Value::Sequence(items)) = cm.get(Value::String("expected".into())) {
            for entry in items {
                if let Value::Mapping(em) = entry
                    && em.len() != 1
                {
                    findings.push(ValidationFinding {
                        severity: Severity::Error,
                        check: "expected_format".into(),
                        file: file.into(),
                        message: format!("case '{name}' has expected entry with multiple keys"),
                    });
                    break;
                }
            }
        }

        // assertion_coverage: every referenced id must exist in the assertions
        // map (only when assertion data is available).
        if assertions_active {
            for aid in &source_ids {
                if !assertions.contains_key(aid) {
                    findings.push(ValidationFinding {
                        severity: Severity::Error,
                        check: "assertion_coverage".into(),
                        file: file.into(),
                        message: format!("assertion '{aid}' referenced in case '{name}' not found in assertions dir"),
                    });
                }
            }
        }

        // level_correctness: a case that declares a level must match the level
        // of each assertion it references.
        if assertions_active && let Some(case_level_raw) = level {
            let case_level = normalize_level(case_level_raw);
            for aid in &source_ids {
                if let Some(a) = assertions.get(aid)
                    && a.level != case_level
                {
                    findings.push(ValidationFinding {
                        severity: Severity::Warn,
                        check: "level_correctness".into(),
                        file: file.into(),
                        message: format!(
                            "case '{name}' has level '{case_level}' but assertion '{aid}' is level '{}'",
                            a.level
                        ),
                    });
                }
            }
        }

        // mixed_level_bundle: a case must not bundle both MUST and MAY
        // assertions.
        if assertions_active {
            let mut levels: BTreeSet<String> = BTreeSet::new();
            for aid in &source_ids {
                if let Some(a) = assertions.get(aid) {
                    levels.insert(a.level.clone());
                }
            }
            if levels.contains("must") && levels.contains("may") {
                findings.push(ValidationFinding {
                    severity: Severity::Warn,
                    check: "mixed_level_bundle".into(),
                    file: file.into(),
                    message: format!("case '{name}' bundles assertions of mixed levels (must and may)"),
                });
            }
        }

        // 12. runnable_expected: runnable case must have expected or steps with expected.
        let has_expected = cm.get(Value::String("expected".into())).is_some();
        let has_steps_with_expected = cm
            .get(Value::String("steps".into()))
            .and_then(|v| v.as_sequence())
            .is_some_and(|steps| {
                steps.iter().any(|step| {
                    step.as_mapping()
                        .is_some_and(|sm| sm.get(Value::String("expected".into())).is_some())
                })
            });
        if !has_expected && !has_steps_with_expected {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                check: "runnable_expected".into(),
                file: file.into(),
                message: format!("case '{name}' has no expected or steps"),
            });
        }

        // negative_coverage tracking: record every referenced assertion id and
        // whether any referencing case is a negative case.
        let is_negative = cm.get(Value::String("negative".into())).and_then(Value::as_bool).unwrap_or(false);
        for aid in &source_ids {
            let entry = referenced_ids.entry(aid.clone()).or_insert(false);
            if is_negative {
                *entry = true;
            }
        }
    }

    // negative_coverage: a negatable assertion that is referenced but never by a
    // negative case is flagged. BTreeMap iteration keeps output sorted by id.
    if assertions_active {
        for (aid, has_negative) in &referenced_ids {
            if let Some(a) = assertions.get(aid)
                && a.negatable
                && !*has_negative
            {
                findings.push(ValidationFinding {
                    severity: Severity::Warn,
                    check: "negative_coverage".into(),
                    file: file.into(),
                    message: format!("assertion '{aid}' is negatable but has no negative test case"),
                });
            }
        }
    }

    // Source-level checks (opt-in via --check-source)
    if check_source {
        run_source_checks(spec, file, findings);
    }
}

// ---------------------------------------------------------------------------
// Source-level checks
// ---------------------------------------------------------------------------

fn run_source_checks(spec: &Value, file: &str, findings: &mut Vec<ValidationFinding>) {
    let Some(map) = spec.as_mapping() else { return };

    let Some(binding_rel) = map.get(Value::String("binding".into())).and_then(|v| v.as_str()) else {
        return;
    };

    let spec_dir = Path::new(file).parent().unwrap_or(Path::new(""));
    let binding_path = spec_dir.join(binding_rel);

    let Ok(binding_raw) = std::fs::read_to_string(&binding_path) else {
        return;
    };
    let binding: Value = match serde_yaml::from_str(&binding_raw) {
        Ok(v) => v,
        Err(_) => return,
    };

    let package_root_rel = binding
        .as_mapping()
        .and_then(|bm| bm.get(Value::String("targets".into())))
        .and_then(|v| v.as_mapping())
        .and_then(|targets| targets.iter().next())
        .and_then(|(_, tv)| tv.as_mapping())
        .and_then(|tm| tm.get(Value::String("package_root".into())))
        .and_then(|v| v.as_str())
        .unwrap_or("src");

    let binding_dir = binding_path.parent().unwrap_or(Path::new(""));
    let package_root = binding_dir.join(package_root_rel);

    if !package_root.exists() {
        return;
    }

    let rs_content = collect_rs_content(&package_root);

    // (F) source_setup_visibility: every `#[spec_setup(...)]` function must be
    // declared `pub fn`.
    let setup_re = Regex::new(r"(?s)#\[spec_setup[^\]]*\]\s*(pub\s+)?fn\s+([A-Za-z_]\w*)").unwrap();
    for caps in setup_re.captures_iter(&rs_content) {
        let is_pub = caps.get(1).is_some();
        if !is_pub {
            let fname = &caps[2];
            findings.push(ValidationFinding {
                severity: Severity::Error,
                check: "source_setup_visibility".into(),
                file: file.into(),
                message: format!("#[spec_setup] function '{fname}' must be declared 'pub fn'"),
            });
        }
    }

    // (G) source_field_visibility: every field of an operation's input type
    // (when that type is also declared in the spec's `types`) must be `pub`.
    let declared_types: BTreeSet<String> = map
        .get(Value::String("types".into()))
        .and_then(|v| v.as_mapping())
        .map(|tm| tm.iter().filter_map(|(k, _)| k.as_str().map(String::from)).collect())
        .unwrap_or_default();

    let mut input_types: Vec<String> = Vec::new();
    if let Some(Value::Mapping(ops_map)) = map.get(Value::String("operations".into())) {
        for (_, v) in ops_map {
            let Some(body) = v.as_mapping() else { continue };
            if let Some(Value::Mapping(inputs)) = body.get(Value::String("inputs".into())) {
                for (_, tv) in inputs {
                    if let Some(type_name) = tv.as_str()
                        && declared_types.contains(type_name)
                        && !input_types.iter().any(|t| t == type_name)
                    {
                        input_types.push(type_name.to_string());
                    }
                }
            }
        }
    }

    let field_re = Regex::new(r"(?m)^\s*(pub\s+)?([A-Za-z_]\w*)\s*:").unwrap();
    for type_name in &input_types {
        let pat = format!(r"struct\s+{}\s*\{{([^}}]*)\}}", regex::escape(type_name));
        let Ok(struct_re) = Regex::new(&pat) else {
            continue;
        };
        if let Some(caps) = struct_re.captures(&rs_content) {
            let body = &caps[1];
            for fcaps in field_re.captures_iter(body) {
                let is_pub = fcaps.get(1).is_some();
                if !is_pub {
                    let fname = &fcaps[2];
                    findings.push(ValidationFinding {
                        severity: Severity::Error,
                        check: "source_field_visibility".into(),
                        file: file.into(),
                        message: format!("field '{fname}' of input type '{type_name}' must be 'pub'"),
                    });
                }
            }
        }
    }
}

fn collect_rs_content(dir: &Path) -> String {
    let mut content = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs")
                && let Ok(s) = std::fs::read_to_string(&p)
            {
                content.push_str(&s);
                content.push('\n');
            }
        }
    }
    content
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const HINTS: &[&str] = &[
    "confirm", "returns", "return ", "rejects", "reject ", "should ", "produces", "outputs", "asserts", "must ",
];

fn looks_testable(s: &str) -> bool {
    let lower = s.to_lowercase();
    HINTS.iter().any(|h| lower.contains(h))
}

/// Render a validate outcome to a colored, human-readable string for
/// terminal display.
#[must_use]
pub fn format_outcome(outcome: &ValidateOutcome) -> String {
    let report = match outcome {
        ValidateOutcome::Pass { report } | ValidateOutcome::Fail { report } => report,
    };
    let mut s = String::new();
    for f in &report.findings {
        let color = match f.severity {
            Severity::Error => "\x1b[31m",
            Severity::Warn => "\x1b[33m",
            Severity::Info => "\x1b[36m",
        };
        let _ = writeln!(s, "{}{}\x1b[0m [{}] {}: {}", color, f.severity.as_str(), f.check, f.file, f.message);
    }
    let _ = writeln!(
        s,
        "files: {}  errors: {}  warnings: {}",
        report.total_files, report.errors, report.warnings
    );
    s
}
