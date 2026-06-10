use std::collections::HashMap;

use crate::diagnostics::*;
use crate::types::*;

pub struct ValidationResult {
    pub operations: Vec<ValidatedOperation>,
    pub report: DiagnosticReport,
}

pub fn validate(extraction: &ExtractionResult) -> ValidationResult {
    let mut diagnostics = Vec::new();
    let type_map: HashMap<&str, &TypeInfo> =
        extraction.types.iter().map(|t| (t.name.as_str(), t)).collect();

    // Collect all [SpecGenerator] registrations: type_name → symbol
    let generators: HashMap<&str, &AnnotatedSymbol> = extraction
        .symbols
        .iter()
        .filter(|s| {
            s.annotations
                .iter()
                .any(|a| a.attribute == SpecAttribute::SpecGenerator)
        })
        .filter_map(|s| {
            let gen_ann = s.annotations.iter().find(|a| a.attribute == SpecAttribute::SpecGenerator)?;
            let tn = gen_ann.args.type_name.as_deref()?;
            Some((tn, s))
        })
        .collect();

    // Validate generators are static
    for (type_name, sym) in &generators {
        if !sym.is_static {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::SG006,
                severity: Severity::Error,
                message: format!("[SpecGenerator] for '{type_name}' must be a static method"),
                operation: None,
                symbol: Some(sym.name.clone()),
                location: sym.location.as_ref().map(loc_to_diag),
            });
        }
    }

    // Validate [SpecContext] methods are static
    for sym in &extraction.symbols {
        if sym.annotations.iter().any(|a| a.attribute == SpecAttribute::SpecContext) && !sym.is_static {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::SG007,
                severity: Severity::Error,
                message: "[SpecContext] must be a static method".to_string(),
                operation: None,
                symbol: Some(sym.name.clone()),
                location: sym.location.as_ref().map(loc_to_diag),
            });
        }
    }

    // Group annotations by operation name
    let mut op_map: HashMap<String, OperationBuilder> = HashMap::new();

    for sym in &extraction.symbols {
        for ann in &sym.annotations {
            if ann.attribute == SpecAttribute::SpecGenerator || ann.attribute == SpecAttribute::SpecContext {
                continue; // Not operation-scoped
            }

            let op_name = match &ann.args.name {
                Some(n) => n.clone(),
                None => {
                    diagnostics.push(Diagnostic {
                        code: DiagnosticCode::SG010,
                        severity: Severity::Error,
                        message: format!("[{}] missing required 'name' argument", attr_display(ann.attribute)),
                        operation: None,
                        symbol: Some(sym.name.clone()),
                        location: sym.location.as_ref().map(loc_to_diag),
                    });
                    continue;
                }
            };

            let builder = op_map.entry(op_name.clone()).or_insert_with(|| OperationBuilder {
                name: op_name.clone(),
                kind: None,
                entries: Vec::new(),
            });

            if ann.attribute == SpecAttribute::SpecOperation {
                match ann.args.kind {
                    Some(kind) => {
                        if let Some(existing) = builder.kind {
                            if existing != kind {
                                diagnostics.push(Diagnostic {
                                    code: DiagnosticCode::SG008,
                                    severity: Severity::Error,
                                    message: format!(
                                        "Operation '{op_name}' declared with conflicting kinds: {existing:?} vs {kind:?}"
                                    ),
                                    operation: Some(op_name.clone()),
                                    symbol: Some(sym.name.clone()),
                                    location: sym.location.as_ref().map(loc_to_diag),
                                });
                            }
                        }
                        builder.kind = Some(kind);
                    }
                    None => {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::SG009,
                            severity: Severity::Error,
                            message: format!("[SpecOperation] on '{op_name}' missing required 'kind' argument"),
                            operation: Some(op_name.clone()),
                            symbol: Some(sym.name.clone()),
                            location: sym.location.as_ref().map(loc_to_diag),
                        });
                    }
                }
            }

            builder.entries.push(AnnotationEntry {
                attribute: ann.attribute,
                symbol: sym.clone(),
                dep: ann.args.dep.clone(),
            });
        }
    }

    // Check for orphan annotations (reference non-existent operations)
    let op_names: std::collections::HashSet<&str> = op_map
        .values()
        .filter(|b| b.kind.is_some())
        .map(|b| b.name.as_str())
        .collect();

    for (name, builder) in &op_map {
        if builder.kind.is_none() && !op_names.contains(name.as_str()) {
            for entry in &builder.entries {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::SG001,
                    severity: Severity::Error,
                    message: format!(
                        "[{}] references operation '{name}' which has no [SpecOperation]",
                        attr_display(entry.attribute)
                    ),
                    operation: Some(name.clone()),
                    symbol: Some(entry.symbol.name.clone()),
                    location: entry.symbol.location.as_ref().map(loc_to_diag),
                });
            }
        }
    }

    // Build validated operations
    let mut operations = Vec::new();

    for builder in op_map.values() {
        let kind = match builder.kind {
            Some(k) => k,
            None => continue, // Already reported as orphan
        };

        let mut inputs = Vec::new();
        let mut environments = Vec::new();
        let mut dependencies = Vec::new();
        let mut checkpoints = Vec::new();
        let mut states = Vec::new();
        let mut return_type = None;

        for entry in &builder.entries {
            let sym = &entry.symbol;

            match entry.attribute {
                SpecAttribute::SpecOperation => {
                    return_type = sym.return_type.clone();
                }
                SpecAttribute::SpecInput => {
                    validate_role_target(
                        entry.attribute,
                        sym,
                        &[SymbolKind::Constructor, SymbolKind::Property, SymbolKind::Method],
                        &builder.name,
                        &mut diagnostics,
                    );
                    // For constructors, each parameter becomes an input
                    if sym.symbol_kind == SymbolKind::Constructor {
                        for param in &sym.parameters {
                            inputs.push(ResolvedField {
                                name: param.name.clone(),
                                type_name: param.type_name.clone(),
                            });
                        }
                    } else {
                        let tn = sym
                            .type_name
                            .as_deref()
                            .or(sym.return_type.as_deref())
                            .unwrap_or("unknown")
                            .to_string();
                        inputs.push(ResolvedField {
                            name: sym.name.clone(),
                            type_name: tn,
                        });
                    }
                }
                SpecAttribute::SpecCheckpoint => {
                    if sym.symbol_kind != SymbolKind::Method {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::SG011,
                            severity: Severity::Error,
                            message: format!(
                                "[SpecCheckpoint] must be on a method, found on {:?} '{}'",
                                sym.symbol_kind, sym.name
                            ),
                            operation: Some(builder.name.clone()),
                            symbol: Some(sym.name.clone()),
                            location: sym.location.as_ref().map(loc_to_diag),
                        });
                    }
                    let tn = sym.return_type.as_deref().unwrap_or("unknown").to_string();
                    checkpoints.push(ResolvedField {
                        name: sym.name.clone(),
                        type_name: tn,
                    });
                }
                SpecAttribute::SpecState => {
                    if kind != SpecKind::StateMachine {
                        diagnostics.push(Diagnostic {
                            code: DiagnosticCode::SG004,
                            severity: Severity::Warning,
                            message: format!(
                                "[SpecState] on '{}' but operation '{}' is {:?}, not StateMachine",
                                sym.name, builder.name, kind
                            ),
                            operation: Some(builder.name.clone()),
                            symbol: Some(sym.name.clone()),
                            location: sym.location.as_ref().map(loc_to_diag),
                        });
                    }
                    validate_role_target(
                        entry.attribute,
                        sym,
                        &[SymbolKind::Field, SymbolKind::Property],
                        &builder.name,
                        &mut diagnostics,
                    );
                    let tn = sym
                        .type_name
                        .as_deref()
                        .or(sym.return_type.as_deref())
                        .unwrap_or("unknown")
                        .to_string();
                    states.push(ResolvedField {
                        name: sym.name.clone(),
                        type_name: tn,
                    });
                }
                SpecAttribute::SpecEnvironment => {
                    validate_role_target(
                        entry.attribute,
                        sym,
                        &[SymbolKind::Property, SymbolKind::Method],
                        &builder.name,
                        &mut diagnostics,
                    );
                    let tn = sym
                        .type_name
                        .as_deref()
                        .or(sym.return_type.as_deref())
                        .unwrap_or("unknown")
                        .to_string();
                    environments.push(ResolvedField {
                        name: sym.name.clone(),
                        type_name: tn,
                    });
                }
                SpecAttribute::SpecDependency => {
                    validate_role_target(
                        entry.attribute,
                        sym,
                        &[SymbolKind::Property, SymbolKind::Method],
                        &builder.name,
                        &mut diagnostics,
                    );
                    let tn = sym.type_name.as_deref()
                        .or(sym.return_type.as_deref())
                        .unwrap_or("unknown")
                        .to_string();
                    let dep = entry.dep.clone().unwrap_or_default();
                    dependencies.push(ResolvedDependency {
                        name: sym.name.clone(),
                        type_name: tn,
                        dep,
                    });
                }
                _ => {} // SpecGenerator, SpecContext handled separately
            }
        }

        // Completeness: every operation needs at least one input
        if inputs.is_empty() {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::SG003,
                severity: Severity::Warning,
                message: format!("Operation '{}' has no inputs", builder.name),
                operation: Some(builder.name.clone()),
                symbol: None,
                location: None,
            });
        }

        // Constructability: check input and environment types
        for field in inputs.iter().chain(environments.iter()) {
            check_constructability(&field.type_name, &builder.name, &type_map, &generators, &mut diagnostics);
        }

        operations.push(ValidatedOperation {
            name: builder.name.clone(),
            kind,
            inputs,
            environments,
            dependencies,
            checkpoints,
            states,
            return_type,
        });
    }

    let report = DiagnosticReport::new(diagnostics, operations.len());
    ValidationResult { operations, report }
}

// ── Helpers ──

struct OperationBuilder {
    name: String,
    kind: Option<SpecKind>,
    entries: Vec<AnnotationEntry>,
}

struct AnnotationEntry {
    attribute: SpecAttribute,
    symbol: AnnotatedSymbol,
    dep: Option<String>,
}

fn validate_role_target(
    attr: SpecAttribute,
    sym: &AnnotatedSymbol,
    valid_kinds: &[SymbolKind],
    op_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !valid_kinds.contains(&sym.symbol_kind) {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::SG002,
            severity: Severity::Error,
            message: format!(
                "[{}] cannot be placed on {:?} '{}' (valid: {valid_kinds:?})",
                attr_display(attr),
                sym.symbol_kind,
                sym.name,
            ),
            operation: Some(op_name.to_string()),
            symbol: Some(sym.name.clone()),
            location: sym.location.as_ref().map(loc_to_diag),
        });
    }
}

fn check_constructability(
    type_name: &str,
    op_name: &str,
    type_map: &HashMap<&str, &TypeInfo>,
    generators: &HashMap<&str, &AnnotatedSymbol>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Primitives and well-known types are always constructable
    if is_primitive(type_name) {
        return;
    }

    // If there's a generator, it's constructable
    if generators.contains_key(type_name) {
        return;
    }

    match type_map.get(type_name) {
        None => {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::SG012,
                severity: Severity::Note,
                message: format!("Type '{type_name}' referenced but not found in extraction type info"),
                operation: Some(op_name.to_string()),
                symbol: None,
                location: None,
            });
        }
        Some(ti) => {
            if ti.has_spec_generator {
                return;
            }
            if ti.is_abstract {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::SG005,
                    severity: Severity::Error,
                    message: format!(
                        "Type '{type_name}' is abstract/interface — needs a [SpecGenerator]"
                    ),
                    operation: Some(op_name.to_string()),
                    symbol: None,
                    location: None,
                });
                return;
            }
            let has_public_ctor = ti.constructors.iter().any(|c| {
                c.accessibility == Some(Accessibility::Public)
                    || c.accessibility == Some(Accessibility::Internal)
            });
            if !has_public_ctor && !ti.constructors.is_empty() {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::SG005,
                    severity: Severity::Error,
                    message: format!(
                        "Type '{type_name}' has no public constructor — needs a [SpecGenerator]"
                    ),
                    operation: Some(op_name.to_string()),
                    symbol: None,
                    location: None,
                });
            }
        }
    }
}

fn is_primitive(type_name: &str) -> bool {
    matches!(
        type_name,
        "string" | "String" | "str" | "int" | "Int32" | "i32" | "i64" | "u32" | "u64"
            | "bool" | "Boolean" | "float" | "double" | "f32" | "f64"
            | "char" | "byte" | "Guid"
    )
}

fn attr_display(attr: SpecAttribute) -> &'static str {
    match attr {
        SpecAttribute::SpecOperation => "SpecOperation",
        SpecAttribute::SpecInput => "SpecInput",
        SpecAttribute::SpecCheckpoint => "SpecCheckpoint",
        SpecAttribute::SpecState => "SpecState",
        SpecAttribute::SpecEnvironment => "SpecEnvironment",
        SpecAttribute::SpecDependency => "SpecDependency",
        SpecAttribute::SpecGenerator => "SpecGenerator",
        SpecAttribute::SpecContext => "SpecContext",
    }
}

fn loc_to_diag(loc: &SourceLocation) -> DiagnosticLocation {
    DiagnosticLocation {
        file: loc.file.clone(),
        line: loc.line,
    }
}
