//! Normalized semantic discovery models.
//!
//! # Example
//! ```no_run
//! use specgate_discovery::schema::Operation;
//! let operation = Operation::builder("health").build();
//! assert!(operation.output.is_none());
//! ```
//!
//! Schemas and unknown producer kinds retain strong semantic identities:
//!
//! ```
//! use specgate_discovery::schema::UnknownKind;
//! assert!(UnknownKind::new("struct").is_err());
//! assert_eq!(UnknownKind::new("future-kind")?.as_str(), "future-kind");
//! # Ok::<(), specgate_discovery::Error>(())
//! ```
//!
//! Batch callers can distinguish absent, valid, and invalid component schemas:
//!
//! ```no_run
//! use specgate_discovery::output::{Batch, SchemaLookup};
//! # fn inspect(batch: &Batch) {
//! match batch.schema("demo") {
//!     SchemaLookup::Found(schema) => assert_eq!(schema.component.as_str(), "demo"),
//!     SchemaLookup::Missing | SchemaLookup::Invalid(_) => {}
//! }
//! # }
//! ```

use std::collections::BTreeMap;

use super::registry::Registry;
use crate::error::{Error, ErrorKind};
use crate::identity::{ComponentId, ErrorName, FieldName, OperationName, TypeExpression, TypeName, VariantName};
use std::fmt;

/// One black-box operation input, with any setup construction params folded in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Input {
    /// Semantic input name.
    pub name: FieldName,
    /// Normalized spec type, e.g. `"i32"`, `"List<i32>"`, `"Option<i32>"`.
    pub ty: TypeExpression,
}

/// One operation on the component's normalized surface.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Operation {
    /// Semantic operation name.
    pub name: OperationName,
    /// Whether the implementation operation is asynchronous.
    pub is_async: bool,
    /// Folded semantic inputs.
    pub inputs: Vec<Input>,
    /// Normalized semantic return type, or `None` when it returns unit.
    pub output: Option<TypeExpression>,
    /// Whether the operation may complete through CTSC's explicit empty channel.
    pub empty: bool,
    /// Declared semantic errors. Empty when discovery exposes no declaration.
    pub errors: Vec<ErrorDeclaration>,
    /// Setup producers folded into this operation's public input surface.
    pub setups: Vec<Setup>,
}

#[cfg(feature = "serde")]
impl serde::Serialize for Operation {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        const FIELD_COUNT: usize = 7;
        let mut state = serializer.serialize_struct("Operation", FIELD_COUNT)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("is_async", &self.is_async)?;
        state.serialize_field("inputs", &self.inputs)?;
        state.serialize_field("output", self.output.as_ref().map_or("", TypeExpression::as_str))?;
        state.serialize_field("empty", &self.empty)?;
        state.serialize_field("errors", &self.errors)?;
        state.serialize_field("setups", &self.setups)?;
        state.end()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
/// One declared operation error channel.
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct ErrorDeclaration {
    /// Declared error name.
    pub name: ErrorName,
    /// Normalized error payload type, or `None` for unit errors.
    pub ty: Option<TypeExpression>,
}

impl fmt::Display for ErrorDeclaration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ty) = &self.ty {
            write!(f, "{}: {ty}", self.name)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

/// One deterministic setup producer associated with an operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Setup {
    /// Operation parameter filled by this setup.
    pub fills: FieldName,
    /// Setup construction inputs.
    pub inputs: Vec<Input>,
    /// Setup output type.
    pub output: TypeExpression,
}

/// One named field (struct field or enum-variant field).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Field {
    /// Semantic field name.
    pub name: FieldName,
    /// Normalized field type.
    pub ty: TypeExpression,
}

/// One enum variant. Named payloads populate `fields`; tuple payloads populate
/// `tuple`; unit variants populate neither.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Variant {
    /// Semantic variant name.
    pub name: VariantName,
    /// Named payload fields.
    pub fields: Vec<Field>,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    /// Ordered tuple payload types.
    pub tuple: Option<Vec<TypeExpression>>,
}

/// Semantic shape of a discovered named type.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "the normalized type-kind protocol is intentionally closed and exhaustively encoded"
)]
pub enum TypeKind {
    /// A record-like type with named fields.
    Struct,
    /// A tagged union with variants.
    Enum,
    /// An unrecognized producer value, retained for wire compatibility.
    Other(UnknownKind),
}

/// A producer type kind not recognized by this discovery version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownKind(String);

impl UnknownKind {
    /// Construct an unknown kind while rejecting recognized reserved values.
    ///
    /// # Errors
    ///
    /// Returns an error for `struct` and `enum`, which have dedicated variants.
    pub fn new(value: impl Into<String>) -> Result<Self, Error> {
        let value = value.into();
        if matches!(value.as_str(), "struct" | "enum") {
            return Err(Error::message(
                ErrorKind::Normalization,
                format!("'{value}' is a recognized type kind"),
            ));
        }
        Ok(Self(value))
    }

    /// Borrow the original producer value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TypeKind {
    pub(crate) fn parse(value: impl AsRef<str>) -> Self {
        match value.as_ref() {
            "struct" => Self::Struct,
            "enum" => Self::Enum,
            other => Self::Other(UnknownKind(other.to_owned())),
        }
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for TypeKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Other(value) => value.as_str(),
        })
    }
}

/// One named complex type owned by the component. `kind` is `"struct"` or
/// `"enum"`; structs populate `fields`, enums populate `variants`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct TypeDef {
    /// Semantic type name.
    pub name: TypeName,
    /// Type kind: `struct` or `enum`.
    pub kind: TypeKind,
    /// Struct fields.
    pub fields: Vec<Field>,
    /// Enum variants.
    pub variants: Vec<Variant>,
}

/// Required identities for [`TypeDef::builder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDefDeps {
    /// Semantic type name.
    pub name: TypeName,
    /// Type kind.
    pub kind: TypeKind,
}

/// Staged construction for a normalized type declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct TypeDefBuilder {
    declaration: TypeDef,
}

impl TypeDef {
    /// Start a declaration builder with the required type identity.
    pub fn builder(deps: impl Into<TypeDefDeps>) -> TypeDefBuilder {
        let deps = deps.into();
        TypeDefBuilder {
            declaration: Self {
                name: deps.name,
                kind: deps.kind,
                fields: Vec::new(),
                variants: Vec::new(),
            },
        }
    }
}

impl TypeDefBuilder {
    /// Set struct fields.
    pub fn fields(mut self, fields: impl Into<Vec<Field>>) -> Self {
        let mut fields = fields.into();
        fields.shrink_to_fit();
        self.declaration.fields = fields;
        self
    }

    /// Set enum variants.
    pub fn variants(mut self, variants: impl Into<Vec<Variant>>) -> Self {
        let mut variants = variants.into();
        variants.shrink_to_fit();
        self.declaration.variants = variants;
        self
    }

    /// Finish the declaration after checking kind-specific payloads.
    ///
    /// # Errors
    ///
    /// Returns a normalization error when a struct declares enum variants or
    /// an enum declares struct fields.
    pub fn build(self) -> Result<TypeDef, Error> {
        match self.declaration.kind {
            TypeKind::Struct if !self.declaration.variants.is_empty() => {
                return Err(Error::message(
                    ErrorKind::Normalization,
                    format!("struct type '{}' cannot declare enum variants", self.declaration.name),
                ));
            }
            TypeKind::Enum if !self.declaration.fields.is_empty() => {
                return Err(Error::message(
                    ErrorKind::Normalization,
                    format!("enum type '{}' cannot declare struct fields", self.declaration.name),
                ));
            }
            TypeKind::Struct | TypeKind::Enum | TypeKind::Other(_) => {}
        }
        Ok(self.declaration)
    }
}

/// A component's normalized, folded schema.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Schema {
    /// Component identifier.
    pub component: ComponentId,
    /// Direct component dependencies.
    pub dependencies: Vec<ComponentId>,
    /// Referenced dependency type declarations.
    pub dependency_types: Vec<Dependency>,
    /// Normalized operation declarations.
    pub operations: Vec<Operation>,
    /// Component-owned named types.
    pub types: Vec<TypeDef>,
}

/// Staged construction for a normalized component schema.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct SchemaBuilder {
    schema: Schema,
}

impl Schema {
    /// Begin a schema with its required component identity and empty declarations.
    pub fn builder(component: impl Into<ComponentId>) -> SchemaBuilder {
        SchemaBuilder {
            schema: Self {
                component: component.into(),
                dependencies: Vec::new(),
                dependency_types: Vec::new(),
                operations: Vec::new(),
                types: Vec::new(),
            },
        }
    }
}

impl SchemaBuilder {
    /// Set direct component dependencies.
    pub fn dependencies(mut self, dependencies: impl IntoIterator<Item = ComponentId>) -> Self {
        let mut values = dependencies.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.dependencies = values;
        self
    }
    /// Set referenced dependency type declarations.
    pub fn dependency_types(mut self, dependency_types: impl IntoIterator<Item = Dependency>) -> Self {
        let mut values = dependency_types.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.dependency_types = values;
        self
    }
    /// Set normalized operation declarations.
    pub fn operations(mut self, operations: impl IntoIterator<Item = Operation>) -> Self {
        let mut values = operations.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.operations = values;
        self
    }
    /// Set component-owned named types.
    pub fn types(mut self, types: impl IntoIterator<Item = TypeDef>) -> Self {
        let mut values = types.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.types = values;
        self
    }
    /// Finish the normalized schema.
    #[must_use]
    pub fn build(self) -> Schema {
        self.schema
    }
}

/// Referenced named types owned by one dependency component.
///
/// These resolve component-qualified references in standalone registry exports.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[expect(
    clippy::exhaustive_structs,
    reason = "normalized schema DTOs are intentionally constructible and destructurable by consumers"
)]
pub struct Dependency {
    /// Dependency component identifier.
    pub component: ComponentId,
    /// Dependency's direct dependencies.
    pub dependencies: Vec<ComponentId>,
    /// Referenced types owned by the dependency.
    pub types: Vec<TypeDef>,
}

/// Staged construction for referenced dependency types.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct DependencyBuilder {
    schema: Dependency,
}

impl Dependency {
    /// Begin a dependency schema with its required component identity.
    pub fn builder(component: impl Into<ComponentId>) -> DependencyBuilder {
        DependencyBuilder {
            schema: Self {
                component: component.into(),
                dependencies: Vec::new(),
                types: Vec::new(),
            },
        }
    }
}

impl DependencyBuilder {
    /// Set direct dependencies of the referenced component.
    pub fn dependencies(mut self, dependencies: impl IntoIterator<Item = ComponentId>) -> Self {
        let mut values = dependencies.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.dependencies = values;
        self
    }

    /// Set referenced type declarations.
    pub fn types(mut self, types: impl IntoIterator<Item = TypeDef>) -> Self {
        let mut values = types.into_iter().collect::<Vec<_>>();
        values.shrink_to_fit();
        self.schema.types = values;
        self
    }

    /// Finish the dependency schema.
    #[must_use]
    pub fn build(self) -> Dependency {
        self.schema
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_def_rejects_kind_payload_mismatches() {
        let struct_result = TypeDef::builder(TypeDefDeps {
            name: "Record".into(),
            kind: TypeKind::Struct,
        })
        .variants(vec![Variant {
            name: "Unexpected".into(),
            fields: Vec::new(),
            tuple: None,
        }])
        .build();
        assert!(matches!(struct_result, Err(ref error) if error.is_normalization()));

        let enum_result = TypeDef::builder(TypeDefDeps {
            name: "Choice".into(),
            kind: TypeKind::Enum,
        })
        .fields(vec![Field {
            name: "unexpected".into(),
            ty: "string".into(),
        }])
        .build();
        assert!(matches!(enum_result, Err(ref error) if error.is_normalization()));
    }

    #[test]
    fn dependency_builder_collects_referenced_types() {
        let ty = TypeDef::builder(TypeDefDeps {
            name: "Shared".into(),
            kind: TypeKind::Struct,
        })
        .build()
        .expect("empty struct declaration should be valid");
        let dependency = Dependency::builder("shared.types")
            .dependencies(["shared.base".into()])
            .types([ty.clone()])
            .build();

        assert_eq!(dependency.component.as_str(), "shared.types");
        assert_eq!(dependency.dependencies[0].as_str(), "shared.base");
        assert_eq!(dependency.types, vec![ty]);
    }
}

/// Raw and normalized metadata for one selected binding target.
#[derive(Debug, Clone)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Target {
    /// Resolved binding target.
    #[expect(clippy::struct_field_names, reason = "target is the domain term for this binding value")]
    pub target: crate::binding::ResolvedTarget,
    /// Candidate Cargo identity for Rust targets.
    pub cargo_context: Option<crate::runner::CandidatePackage>,
    /// Raw registry JSON emitted by the target.
    pub registry_json: String,
    /// Parsed raw registry. This producer-interoperability surface is available
    /// only with the `raw-registry` feature.
    #[cfg(feature = "raw-registry")]
    pub registry: Registry,
    #[cfg(not(feature = "raw-registry"))]
    pub(crate) registry: Registry,
    /// Normalized selected component schema.
    pub schema: Schema,
}

/// Raw registry selection and normalized schema for one requested component.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Component {
    /// Index of this component's document in [`Batch::registries`].
    pub registry_index: usize,
    /// Normalized schema, or this component's exact normalization failure.
    pub schema: Result<Schema, Error>,
}

/// Outcome of looking up one component in a discovery batch.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_enums,
    reason = "schema lookup has a closed result taxonomy exhaustively handled by callers"
)]
pub enum SchemaLookup<'a> {
    /// The component was not requested.
    Missing,
    /// The component normalized successfully.
    Found(&'a Schema),
    /// The component was requested but normalization failed.
    Invalid(&'a Error),
}

/// Raw and normalized metadata for many components of one binding target.
///
/// Rust targets link and self-report once, so every component shares registry
/// index `0`. C# targets build and reflect once, emitting one raw document per
/// component. Either way, the expensive toolchain work happens a single time.
#[derive(Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "discovery result DTOs expose their complete evidence to workspace consumers"
)]
pub struct Batch {
    /// Resolved binding target.
    pub target: crate::binding::ResolvedTarget,
    /// Candidate Cargo identity for Rust targets.
    pub cargo_context: Option<crate::runner::CandidatePackage>,
    /// Raw registry documents in emission order.
    pub registry_json: Vec<String>,
    /// Parsed registries aligned with `registry_json`. This producer-
    /// interoperability surface is available only with `raw-registry`.
    #[cfg(feature = "raw-registry")]
    pub registries: Vec<Registry>,
    #[cfg(not(feature = "raw-registry"))]
    pub(crate) registries: Vec<Registry>,
    /// Requested components, sorted and deduplicated.
    pub components: BTreeMap<ComponentId, Component>,
    /// Every component the compiled target declares an operation for, sorted
    /// and deduplicated, independent of what this run requested. Callers that
    /// must account for the whole target compare their expected coverage
    /// against this rather than against the requested subset.
    pub present_components: Vec<ComponentId>,
}

impl Batch {
    /// The parsed registry backing `component`.
    ///
    /// # Panics
    ///
    /// Panics if internal component metadata references no backing registry.
    #[must_use]
    pub fn registry(&self, component: impl AsRef<str>) -> Option<&Registry> {
        let component = component.as_ref();
        self.components.get(component).map(|discovered| {
            let index = discovered.registry_index;
            let count = self.registries.len();
            self.registries
                .get(index)
                .unwrap_or_else(|| panic!("component registry index {index} must reference one of {count} registries"))
        })
    }

    /// The raw registry document backing `component`.
    ///
    /// # Panics
    /// Panics if internal component metadata references no backing registry document.
    #[must_use]
    pub fn registry_json(&self, component: impl AsRef<str>) -> Option<&str> {
        let component = component.as_ref();
        self.components.get(component).map(|discovered| {
            let index = discovered.registry_index;
            let count = self.registry_json.len();
            self.registry_json
                .get(index)
                .unwrap_or_else(|| panic!("component registry JSON index {index} must reference one of {count} documents"))
                .as_str()
        })
    }

    /// The normalized schema result for `component`.
    #[must_use]
    pub fn schema(&self, component: impl AsRef<str>) -> SchemaLookup<'_> {
        let component = component.as_ref();
        match self.components.get(component).map(|discovered| &discovered.schema) {
            None => SchemaLookup::Missing,
            Some(Ok(schema)) => SchemaLookup::Found(schema),
            Some(Err(error)) => SchemaLookup::Invalid(error),
        }
    }
}

/// Builder for a normalized discovered operation.
#[must_use]
#[derive(Debug)]
pub struct OperationBuilder {
    operation: Operation,
}

impl Operation {
    /// Start a builder with the required semantic name.
    pub fn builder(name: impl Into<OperationName>) -> OperationBuilder {
        OperationBuilder {
            operation: Self {
                name: name.into(),
                is_async: false,
                inputs: Vec::new(),
                output: None,
                empty: false,
                errors: Vec::new(),
                setups: Vec::new(),
            },
        }
    }
}

impl OperationBuilder {
    /// Set whether the operation is asynchronous.
    pub const fn asynchronous(mut self, value: bool) -> Self {
        self.operation.is_async = value;
        self
    }
    /// Set folded inputs.
    pub fn inputs(mut self, mut value: Vec<Input>) -> Self {
        value.shrink_to_fit();
        self.operation.inputs = value;
        self
    }
    /// Set the normalized output type.
    pub fn output(mut self, value: impl Into<TypeExpression>) -> Self {
        self.operation.output = Some(value.into());
        self
    }
    /// Set whether explicit empty completion is permitted.
    pub const fn empty(mut self, value: bool) -> Self {
        self.operation.empty = value;
        self
    }
    /// Set declared errors.
    pub fn errors(mut self, mut value: Vec<ErrorDeclaration>) -> Self {
        value.shrink_to_fit();
        self.operation.errors = value;
        self
    }
    /// Set folded setups.
    pub fn setups(mut self, mut value: Vec<Setup>) -> Self {
        value.shrink_to_fit();
        self.operation.setups = value;
        self
    }
    /// Build the operation.
    #[must_use]
    pub fn build(self) -> Operation {
        self.operation
    }
}
