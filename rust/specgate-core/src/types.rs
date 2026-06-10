use serde::{Deserialize, Serialize};

// ── Intermediate format (input from language front-ends) ──

#[derive(Debug, Deserialize)]
pub struct ExtractionResult {
    pub source_language: String,
    pub project: Option<String>,
    pub symbols: Vec<AnnotatedSymbol>,
    #[serde(default)]
    pub types: Vec<TypeInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AnnotatedSymbol {
    pub name: String,
    pub symbol_kind: SymbolKind,
    pub declaring_type: Option<String>,
    pub return_type: Option<String>,
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    #[serde(default)]
    pub parameters: Vec<ParameterInfo>,
    pub accessibility: Option<Accessibility>,
    #[serde(default)]
    pub is_static: bool,
    #[serde(default)]
    pub is_async: bool,
    pub location: Option<SourceLocation>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Method,
    Property,
    Field,
    Constructor,
    Parameter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Accessibility {
    Public,
    Internal,
    Protected,
    Private,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Annotation {
    pub attribute: SpecAttribute,
    #[serde(default)]
    pub args: AnnotationArgs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum SpecAttribute {
    SpecOperation,
    SpecInput,
    SpecCheckpoint,
    SpecState,
    SpecEnvironment,
    SpecDependency,
    SpecGenerator,
    SpecContext,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AnnotationArgs {
    pub name: Option<String>,
    pub kind: Option<SpecKind>,
    pub dep: Option<String>,
    pub type_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum SpecKind {
    Pure,
    StateMachine,
    Sequence,
    ErrorMap,
    Structural,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TypeInfo {
    pub name: String,
    #[serde(default)]
    pub is_abstract: bool,
    #[serde(default)]
    pub is_generic: bool,
    #[serde(default)]
    pub generic_parameters: Vec<String>,
    pub accessibility: Option<Accessibility>,
    #[serde(default)]
    pub constructors: Vec<ConstructorInfo>,
    #[serde(default)]
    pub has_spec_generator: bool,
    pub base_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConstructorInfo {
    pub accessibility: Option<Accessibility>,
    #[serde(default)]
    pub parameters: Vec<ParameterInfo>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParameterInfo {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    #[serde(default)]
    pub is_optional: bool,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceLocation {
    pub file: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

// ── Spec output format (matches spec-schema.json) ──

#[derive(Debug, Serialize)]
pub struct SpecFile {
    pub name: String,
    pub kind: SpecKind,
    pub inputs: Vec<TypedField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub environments: Vec<TypedField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<DependencyField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checkpoints: Vec<TypedField>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub states: Vec<TypedField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<serde_yaml::Value>,
}

#[derive(Debug, Serialize)]
pub struct TypedField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
}

#[derive(Debug, Serialize)]
pub struct DependencyField {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub dep: String,
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub oneof: Vec<String>,
}

// ── Validated operation (intermediate between extraction and spec) ──

#[derive(Debug)]
pub struct ValidatedOperation {
    pub name: String,
    pub kind: SpecKind,
    pub inputs: Vec<ResolvedField>,
    pub environments: Vec<ResolvedField>,
    pub dependencies: Vec<ResolvedDependency>,
    pub checkpoints: Vec<ResolvedField>,
    pub states: Vec<ResolvedField>,
    pub return_type: Option<String>,
}

#[derive(Debug)]
pub struct ResolvedField {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug)]
pub struct ResolvedDependency {
    pub name: String,
    pub type_name: String,
    pub dep: String,
}
