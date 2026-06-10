use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub severity: Severity,
    pub message: String,
    pub operation: Option<String>,
    pub symbol: Option<String>,
    pub location: Option<DiagnosticLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DiagnosticCode {
    /// Annotation references an operation name that has no [SpecOperation].
    SG001,
    /// Annotation is on an invalid target for its role.
    SG002,
    /// Operation has no inputs.
    SG003,
    /// [SpecState] used on non-StateMachine operation.
    SG004,
    /// Type is unconstructable: no public constructor and no [SpecGenerator].
    SG005,
    /// [SpecGenerator] method is not static.
    SG006,
    /// [SpecContext] method is not static.
    SG007,
    /// Duplicate operation name with conflicting kinds.
    SG008,
    /// [SpecOperation] missing required `kind` argument.
    SG009,
    /// Annotation missing required `name` argument.
    SG010,
    /// [SpecCheckpoint] on non-method symbol.
    SG011,
    /// Type referenced but not found in type info.
    SG012,
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticLocation {
    pub file: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticReport {
    pub diagnostics: Vec<Diagnostic>,
    pub summary: DiagnosticSummary,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticSummary {
    pub errors: usize,
    pub warnings: usize,
    pub notes: usize,
    pub operations_found: usize,
}

impl DiagnosticReport {
    pub fn new(diagnostics: Vec<Diagnostic>, operations_found: usize) -> Self {
        let errors = diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
        let warnings = diagnostics.iter().filter(|d| d.severity == Severity::Warning).count();
        let notes = diagnostics.iter().filter(|d| d.severity == Severity::Note).count();
        Self {
            diagnostics,
            summary: DiagnosticSummary { errors, warnings, notes, operations_found },
        }
    }

    pub fn has_errors(&self) -> bool {
        self.summary.errors > 0
    }

    pub fn has_warnings(&self) -> bool {
        self.summary.warnings > 0
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sev = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Note => "note",
        };

        write!(f, "{sev}[{}]", self.code)?;

        if let Some(loc) = &self.location {
            if let Some(file) = &loc.file {
                write!(f, " {file}")?;
                if let Some(line) = loc.line {
                    write!(f, ":{line}")?;
                }
            }
        }

        write!(f, ": {}", self.message)?;

        if let Some(op) = &self.operation {
            write!(f, " (operation: {op})")?;
        }

        Ok(())
    }
}

impl fmt::Display for DiagnosticReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for d in &self.diagnostics {
            writeln!(f, "{d}")?;
        }
        writeln!(
            f,
            "\n{} error(s), {} warning(s), {} note(s) — {} operation(s) found",
            self.summary.errors, self.summary.warnings, self.summary.notes, self.summary.operations_found
        )
    }
}
