//! `specgate validate <spec-dir>` — static checks across one or more
//! `.spec.yaml` files in a directory tree.

use serde_yaml::Value;
use specgate_annotations::spec_operation;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warn,
    Info,
}

impl Severity {
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
pub fn validate(spec_dir: &str, strict: bool, suppress: &[String]) -> ValidateOutcome {
    let suppress_set: HashSet<String> = suppress.iter().cloned().collect();
    let mut findings: Vec<ValidationFinding> = Vec::new();
    let mut total_files = 0;

    let files = collect_spec_files(Path::new(spec_dir));
    for path in files {
        total_files += 1;
        let file_str = path.to_string_lossy().to_string();
        let raw = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let value: Value = match serde_yaml::from_str(&raw) {
            Ok(v) => v,
            Err(_) => {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    check: "schema".into(),
                    file: file_str.clone(),
                    message: "spec file is not valid YAML".into(),
                });
                continue;
            }
        };
        check_file(&value, &file_str, &mut findings);
    }

    // Apply suppression by removing matching findings.
    findings.retain(|f| !suppress_set.contains(&f.check));

    // In strict mode, upgrade warns to errors.
    if strict {
        for f in findings.iter_mut() {
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
        } else if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.ends_with(".spec.yaml") {
                out.push(p);
            }
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

fn check_file(spec: &Value, file: &str, findings: &mut Vec<ValidationFinding>) {
    let map = match spec.as_mapping() {
        Some(m) => m,
        None => {
            findings.push(ValidationFinding {
                severity: Severity::Error,
                check: "schema".into(),
                file: file.into(),
                message: "spec top-level is not a mapping".into(),
            });
            return;
        }
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
            if let Some(body) = v.as_mapping() {
                if let Some(Value::Mapping(inputs)) = body.get(Value::String("inputs".into())) {
                    for (ik, _) in inputs {
                        if let Some(s) = ik.as_str() {
                            decl.declared_inputs.push(s.to_string());
                        }
                    }
                }
            }
            ops.insert(name.to_string(), decl);
        }
    }

    let cases_v = map.get(Value::String("cases".into()));
    let cases_seq: Vec<&Value> = match cases_v.and_then(|c| c.as_sequence()) {
        Some(s) => s.iter().collect(),
        None => Vec::new(),
    };

    // For negative_coverage: track per-operation MUST-with-source presence and
    // whether any case looks like an error/rejection case.
    let mut op_must_with_source: BTreeMap<String, bool> = BTreeMap::new();
    let mut op_has_negative: BTreeMap<String, bool> = BTreeMap::new();
    let mut seen_names: BTreeSet<String> = BTreeSet::new();

    for c in &cases_seq {
        let Some(cm) = c.as_mapping() else { continue };
        let name = cm
            .get(Value::String("name".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let kind = cm
            .get(Value::String("kind".into()))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let is_narrative = kind == "narrative";
        let level = cm
            .get(Value::String("level".into()))
            .and_then(|v| v.as_str());
        let operation = cm
            .get(Value::String("operation".into()))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

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
            .map(|s| {
                s.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if is_narrative {
            // 7. narrative_misuse: if verify steps look testable, warn.
            let verify = cm
                .get(Value::String("verify".into()))
                .and_then(|v| v.as_sequence());
            if let Some(verify) = verify {
                let any_testable = verify.iter().any(|step| {
                    if let Some(s) = step.as_str() {
                        looks_testable(s)
                    } else {
                        false
                    }
                });
                if any_testable {
                    findings.push(ValidationFinding {
                        severity: Severity::Warn,
                        check: "narrative_misuse".into(),
                        file: file.into(),
                        message: format!(
                            "narrative case '{name}' has verify steps that look testable"
                        ),
                    });
                }
            }
            // Narrative cases skip the runnable-case checks below.
            continue;
        }

        // 2. operation_reference
        if let Some(op) = operation.as_deref() {
            if !ops.contains_key(op) {
                findings.push(ValidationFinding {
                    severity: Severity::Error,
                    check: "operation_reference".into(),
                    file: file.into(),
                    message: format!(
                        "case '{name}' references undefined operation '{op}'"
                    ),
                });
            } else {
                // 5. input_completeness
                let provided: BTreeSet<String> = cm
                    .get(Value::String("inputs".into()))
                    .and_then(|v| v.as_mapping())
                    .map(|m| {
                        m.iter()
                            .filter_map(|(k, _)| k.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let decl = &ops[op];
                for required in &decl.declared_inputs {
                    if !provided.contains(required) {
                        findings.push(ValidationFinding {
                            severity: Severity::Error,
                            check: "input_completeness".into(),
                            file: file.into(),
                            message: format!(
                                "case '{name}' missing required input '{required}' for operation '{op}'"
                            ),
                        });
                    }
                }
            }
        }

        // 4. provenance: every runnable case should have source.assertion_ids.
        if source_ids.is_empty() {
            findings.push(ValidationFinding {
                severity: Severity::Warn,
                check: "provenance".into(),
                file: file.into(),
                message: format!("case '{name}' has no source.assertion_ids"),
            });
        }

        // 6. expected_format: each entry must have exactly one key.
        if let Some(Value::Sequence(items)) = cm.get(Value::String("expected".into())) {
            for entry in items {
                if let Value::Mapping(em) = entry {
                    if em.len() != 1 {
                        findings.push(ValidationFinding {
                            severity: Severity::Error,
                            check: "expected_format".into(),
                            file: file.into(),
                            message: format!(
                                "case '{name}' has expected entry with multiple keys"
                            ),
                        });
                        break;
                    }
                }
            }
        }

        // 9. level_match: if level=may but some source assertion id looks like MUST.
        if level == Some("may") {
            for aid in &source_ids {
                if assertion_id_is_must(aid) {
                    findings.push(ValidationFinding {
                        severity: Severity::Warn,
                        check: "level_match".into(),
                        file: file.into(),
                        message: format!(
                            "case '{name}' has level 'may' but source assertion {aid} appears to be MUST"
                        ),
                    });
                }
            }
        }

        // 10. negative_coverage tracking
        if let Some(op) = operation.as_deref() {
            if level == Some("must") && !source_ids.is_empty() {
                op_must_with_source.insert(op.to_string(), true);
            }
            if case_is_negative(cm, &name) {
                op_has_negative.insert(op.to_string(), true);
            }
        }
    }

    // 10. negative_coverage: emit warnings for ops with MUST cases but no negative case.
    for (op, _) in op_must_with_source.iter() {
        if !op_has_negative.get(op).copied().unwrap_or(false) {
            findings.push(ValidationFinding {
                severity: Severity::Warn,
                check: "negative_coverage".into(),
                file: file.into(),
                message: format!(
                    "operation '{op}' has MUST cases but no error/rejection case"
                ),
            });
        }
    }
}

fn looks_testable(s: &str) -> bool {
    let lower = s.to_lowercase();
    const HINTS: &[&str] = &[
        "confirm", "returns", "return ", "rejects", "reject ", "should ",
        "produces", "outputs", "asserts", "must ",
    ];
    HINTS.iter().any(|h| lower.contains(h))
}

fn assertion_id_is_must(id: &str) -> bool {
    let upper = id.to_uppercase();
    upper.contains("MUST") || upper.contains("SHALL") || upper.contains("REQUIRED")
}

fn case_is_negative(cm: &serde_yaml::Mapping, name: &str) -> bool {
    let lower_name = name.to_lowercase();
    if lower_name.contains("error")
        || lower_name.contains("reject")
        || lower_name.contains("fail")
        || lower_name.contains("invalid")
        || lower_name.contains("bad")
        || lower_name.contains("negative")
    {
        return true;
    }
    if let Some(Value::Sequence(items)) = cm.get(Value::String("expected".into())) {
        for entry in items {
            if let Value::Mapping(em) = entry {
                for (k, v) in em {
                    let kname = k.as_str().unwrap_or("");
                    if kname.ends_with("outcome") {
                        if let Some(val) = v.as_str() {
                            let lv = val.to_lowercase();
                            if lv.contains("error") || lv.contains("reject") || lv.contains("fail")
                            {
                                return true;
                            }
                        }
                    }
                    if kname.ends_with("error") || kname.ends_with(".error") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// Render a validate outcome to a colored, human-readable string for
/// terminal display.
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
        s.push_str(&format!(
            "{}{}\x1b[0m [{}] {}: {}\n",
            color,
            f.severity.as_str(),
            f.check,
            f.file,
            f.message
        ));
    }
    s.push_str(&format!(
        "files: {}  errors: {}  warnings: {}\n",
        report.total_files, report.errors, report.warnings
    ));
    s
}
