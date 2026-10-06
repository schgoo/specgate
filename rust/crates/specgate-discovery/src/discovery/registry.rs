//! Raw discovery registry models and parsing.

use super::BTreeSet;
use crate::identity::{ComponentId, FieldName, FunctionName, KindName, ModulePath, OperationName, TypeExpression, TypeName, VariantName};
use crate::{Error, ErrorKind};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Registry model (parsed from discovery JSON)
// ---------------------------------------------------------------------------

/// C# exception declarations retained from candidate metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum ExceptionMetadata {
    /// The producer supplied no exception metadata.
    #[default]
    Unspecified,
    /// The producer declared a catch-all exception channel.
    CatchAll,
    /// The producer declared these exact exception type names.
    Named(Vec<TypeName>),
}

impl ExceptionMetadata {
    /// Project to the established optional-list representation when adapting protocols.
    #[must_use]
    pub fn as_ref(&self) -> Option<&[TypeName]> {
        match self {
            Self::Unspecified => None,
            Self::CatchAll => Some(&[]),
            Self::Named(names) => Some(names),
        }
    }
}

impl From<Option<Vec<TypeName>>> for ExceptionMetadata {
    fn from(value: Option<Vec<TypeName>>) -> Self {
        match value {
            None => Self::Unspecified,
            Some(names) if names.is_empty() => Self::CatchAll,
            Some(names) => Self::Named(names),
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "raw discovery preserves independent annotation and reflection flags"
)]
#[expect(
    clippy::exhaustive_structs,
    reason = "raw registry DTOs mirror the complete native discovery protocol"
)]
#[derive(Debug, Clone)]
/// Raw operation metadata emitted by native discovery.
pub struct Operation {
    /// Semantic operation or setup name.
    pub name: OperationName,
    /// Native module path.
    pub module_path: ModulePath,
    /// Native function or method name.
    pub fn_name: FunctionName,
    /// Whether this declaration is a setup producer.
    pub is_setup: bool,
    /// Whether this declaration is asynchronous.
    pub is_async: bool,
    /// Whether this declaration is a method.
    pub is_method: bool,
    /// Whether this declaration is public.
    pub is_public: bool,
    /// Raw native return type.
    pub return_type: TypeExpression,
    /// Setup-filled operation parameter.
    pub fills: FieldName,
    /// Raw parameter names and types.
    pub params: Vec<Field>,
    /// Owning component identifier.
    pub component: ComponentId,
    /// C# declaring type, preserved exactly for future candidate replay.
    pub cs_class: Option<TypeName>,
    /// C# declaring type's simple name.
    pub cs_method_of: Option<TypeName>,
    /// C# method name.
    pub cs_method: Option<FunctionName>,
    /// Whether the C# method is static.
    pub cs_is_static: Option<bool>,
    /// Raw C# return signature, including nullable/generic spelling.
    pub cs_return: Option<TypeExpression>,
    /// Raw C# parameter names and signatures.
    pub cs_params: Vec<Field>,
    /// Declared C# exception type names; `None` means no exception metadata,
    /// while `Some([])` is the catch-all declaration.
    pub cs_exceptions: ExceptionMetadata,
}

/// Required semantic identity used to initialize an [`OperationBuilder`].
///
/// ```
/// use specgate_discovery::registry::{OperationId, Operation};
/// let operation = Operation::builder(OperationId::new("demo.component", "health"))
///     .function("health")
///     .build();
/// assert!(operation.is_public);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationId {
    name: OperationName,
    component: ComponentId,
}

impl OperationId {
    /// Create the required identity from its owning component and operation name.
    #[must_use]
    pub fn new(component: impl Into<ComponentId>, name: impl Into<OperationName>) -> Self {
        Self {
            name: name.into(),
            component: component.into(),
        }
    }
}

impl<C, N> From<(C, N)> for OperationId
where
    C: Into<ComponentId>,
    N: Into<OperationName>,
{
    fn from((component, name): (C, N)) -> Self {
        Self::new(component, name)
    }
}

/// Builder for raw operation metadata.
#[must_use]
#[derive(Debug)]
pub struct OperationBuilder {
    operation: Operation,
}

impl Operation {
    /// Start an operation builder with its required semantic identity.
    pub fn builder(required: impl Into<OperationId>) -> OperationBuilder {
        let required = required.into();
        OperationBuilder {
            operation: Self {
                name: required.name,
                component: required.component,
                module_path: String::new().into(),
                fn_name: String::new().into(),
                is_setup: false,
                is_async: false,
                is_method: false,
                is_public: true,
                return_type: String::new().into(),
                fills: String::new().into(),
                params: Vec::new(),
                cs_class: None,
                cs_method_of: None,
                cs_method: None,
                cs_is_static: None,
                cs_return: None,
                cs_params: Vec::new(),
                cs_exceptions: ExceptionMetadata::Unspecified,
            },
        }
    }
}

impl OperationBuilder {
    /// Set the native module path.
    pub fn module_path(mut self, value: impl Into<ModulePath>) -> Self {
        self.operation.module_path = value.into();
        self
    }
    /// Set the native function name.
    pub fn function(mut self, value: impl Into<FunctionName>) -> Self {
        self.operation.fn_name = value.into();
        self
    }
    /// Mark this declaration as a setup producer.
    pub const fn setup(mut self, value: bool) -> Self {
        self.operation.is_setup = value;
        self
    }
    /// Set asynchronous metadata.
    pub const fn asynchronous(mut self, value: bool) -> Self {
        self.operation.is_async = value;
        self
    }
    /// Set method metadata.
    pub const fn method(mut self, value: bool) -> Self {
        self.operation.is_method = value;
        self
    }
    /// Set declaration visibility.
    pub const fn public(mut self, value: bool) -> Self {
        self.operation.is_public = value;
        self
    }
    /// Set the raw return type.
    pub fn return_type(mut self, value: impl Into<TypeExpression>) -> Self {
        self.operation.return_type = value.into();
        self
    }
    /// Set the operation parameter filled by a setup.
    pub fn fills(mut self, value: impl Into<FieldName>) -> Self {
        self.operation.fills = value.into();
        self
    }
    /// Set raw parameter names and types.
    pub fn params(mut self, value: Vec<Field>) -> Self {
        self.operation.params = value;
        self
    }
    /// Set the C# declaring type.
    pub fn csharp_class(mut self, value: Option<TypeName>) -> Self {
        self.operation.cs_class = value;
        self
    }
    /// Set the C# declaring type's simple name.
    pub fn declaring_type(mut self, value: Option<TypeName>) -> Self {
        self.operation.cs_method_of = value;
        self
    }
    /// Set the C# method name.
    pub fn csharp_method(mut self, value: Option<FunctionName>) -> Self {
        self.operation.cs_method = value;
        self
    }
    /// Set whether the C# method is static.
    pub const fn csharp_static(mut self, value: Option<bool>) -> Self {
        self.operation.cs_is_static = value;
        self
    }
    /// Set the raw C# return signature.
    pub fn csharp_return(mut self, value: Option<TypeExpression>) -> Self {
        self.operation.cs_return = value;
        self
    }
    /// Set raw C# parameter names and signatures.
    pub fn csharp_params(mut self, value: Vec<Field>) -> Self {
        self.operation.cs_params = value;
        self
    }
    /// Set declared C# exception metadata.
    pub fn csharp_exceptions(mut self, value: impl Into<ExceptionMetadata>) -> Self {
        self.operation.cs_exceptions = value.into();
        self
    }
    /// Build the raw operation metadata.
    #[must_use]
    pub fn build(self) -> Operation {
        self.operation
    }
}

/// A named raw field or parameter with its producer type spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_structs,
    reason = "raw registry DTOs mirror the complete native discovery protocol"
)]
pub struct Field {
    /// Field or parameter name.
    pub name: FieldName,
    /// Raw producer type expression.
    pub ty: TypeExpression,
}

#[derive(Debug, Clone)]
/// Raw enum-variant metadata emitted by native discovery.
#[expect(
    clippy::exhaustive_structs,
    reason = "raw registry DTOs mirror the complete native discovery protocol"
)]
pub struct Variant {
    /// Variant name.
    pub name: VariantName,
    /// Named payload fields.
    pub fields: Vec<Field>,
    /// Ordered tuple payload types.
    pub tuple: Option<Vec<TypeExpression>>,
}

#[derive(Debug, Clone)]
/// Raw structured-type metadata emitted by native discovery.
#[expect(
    clippy::exhaustive_structs,
    reason = "raw registry DTOs mirror the complete native discovery protocol"
)]
pub struct Type {
    /// Type name.
    pub name: TypeName,
    /// Type kind.
    pub kind: KindName,
    /// Struct fields.
    pub fields: Vec<Field>,
    /// Enum variants.
    pub variants: Vec<Variant>,
    /// Owning component identifier.
    pub component: ComponentId,
}

/// Required identity and kind used to initialize a [`TypeBuilder`].
///
/// ```
/// use specgate_discovery::registry::{Type, TypeId};
/// let metadata = Type::builder(TypeId::new("demo.component", "Status", "enum")).build();
/// assert_eq!(metadata.component, "demo.component");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeId {
    name: TypeName,
    component: ComponentId,
    kind: KindName,
}

impl TypeId {
    /// Create a raw type identity from its component, name, and producer kind.
    #[must_use]
    pub fn new(component: impl Into<ComponentId>, name: impl Into<TypeName>, kind: impl Into<KindName>) -> Self {
        Self {
            name: name.into(),
            component: component.into(),
            kind: kind.into(),
        }
    }
}

/// Builder for raw structured-type metadata.
#[must_use]
#[derive(Debug)]
pub struct TypeBuilder {
    metadata: Type,
}

impl Type {
    /// Start a builder with the required type identity and producer kind.
    pub fn builder(required: impl Into<TypeId>) -> TypeBuilder {
        let required = required.into();
        TypeBuilder {
            metadata: Self {
                name: required.name,
                kind: required.kind,
                fields: Vec::new(),
                variants: Vec::new(),
                component: required.component,
            },
        }
    }
}

impl TypeBuilder {
    /// Set raw struct fields.
    pub fn fields(mut self, fields: Vec<Field>) -> Self {
        self.metadata.fields = fields;
        self
    }

    /// Set raw enum variants.
    pub fn variants(mut self, variants: Vec<Variant>) -> Self {
        self.metadata.variants = variants;
        self
    }

    /// Build the raw type metadata.
    #[must_use]
    pub fn build(self) -> Type {
        self.metadata
    }
}

#[derive(Debug, Clone)]
/// Raw component registry assembled from native discovery metadata.
#[expect(
    clippy::exhaustive_structs,
    reason = "raw registry DTOs mirror the complete native discovery protocol"
)]
pub struct Registry {
    /// Raw operation and setup declarations.
    pub ops: Vec<Operation>,
    /// Raw named-type declarations.
    pub types: Vec<Type>,
}

pub(crate) fn parse(json: impl AsRef<str>) -> Result<Registry, Error> {
    let wire: WireRegistry = serde_json::from_str(json.as_ref())
        .map_err(|source| Error::cause(ErrorKind::Registry, format!("failed to parse discovery JSON: {source}"), source))?;
    let ops = wire.operations.into_iter().map(Operation::from).collect();
    let mut types: Vec<Type> = wire.types.into_iter().map(Type::from).collect();
    types.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Registry { ops, types })
}

impl Registry {
    /// Non-setup operations owned by `comp`, sorted by name.
    #[must_use]
    pub fn operations_for(&self, comp: impl AsRef<str>) -> Vec<&Operation> {
        let comp = comp.as_ref();
        let mut ops: Vec<&Operation> = self.ops.iter().filter(|o| !o.is_setup && o.component == comp).collect();
        ops.sort_by(|a, b| a.name.cmp(&b.name));
        ops.shrink_to_fit();
        ops
    }

    /// Registered types owned by `comp`, sorted by name.
    #[must_use]
    pub fn local_types(&self, comp: impl AsRef<str>) -> Vec<&Type> {
        let comp = comp.as_ref();
        let mut ts: Vec<&Type> = self.types.iter().filter(|t| t.component == comp).collect();
        ts.sort_by(|a, b| a.name.cmp(&b.name));
        ts.shrink_to_fit();
        ts
    }

    /// Distinct, sorted components present among non-setup operations and types.
    #[must_use]
    pub fn present_components(&self) -> Vec<ComponentId> {
        let mut set: BTreeSet<ComponentId> = BTreeSet::new();
        for o in &self.ops {
            if !o.is_setup && !o.component.is_empty() {
                set.insert(o.component.clone());
            }
        }
        for t in &self.types {
            if !t.component.is_empty() {
                set.insert(t.component.clone());
            }
        }
        set.into_iter().collect()
    }

    /// Setups registered for one exact component + operation key.
    #[must_use]
    pub fn setups_for(&self, component: impl AsRef<str>, op: impl AsRef<str>) -> Vec<&Operation> {
        let component = component.as_ref();
        let op = op.as_ref();
        let mut setups = self
            .ops
            .iter()
            .filter(|candidate| candidate.is_setup && candidate.component == component && candidate.name == op)
            .collect::<Vec<_>>();
        setups.shrink_to_fit();
        setups
    }

    /// The names of registered `SpecEvent` types.
    #[must_use]
    pub fn type_names(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }
}

#[derive(Deserialize)]
struct WireRegistry {
    operations: Vec<WireOperation>,
    types: Vec<WireType>,
}

#[derive(Deserialize)]
#[expect(clippy::struct_excessive_bools, reason = "wire fields preserve emitted runtime metadata")]
struct WireOperation {
    name: String,
    component: String,
    #[serde(default)]
    module_path: String,
    #[serde(default)]
    fn_name: String,
    #[serde(default)]
    is_setup: bool,
    #[serde(default)]
    is_async: bool,
    #[serde(default)]
    is_method: bool,
    #[serde(default = "default_public")]
    is_public: bool,
    #[serde(default)]
    return_type: String,
    #[serde(default)]
    fills: String,
    #[serde(default)]
    params: Vec<(String, String)>,
    #[serde(default)]
    cs_class: Option<String>,
    #[serde(default)]
    cs_method_of: Option<String>,
    #[serde(default)]
    cs_method: Option<String>,
    #[serde(default)]
    cs_is_static: Option<bool>,
    #[serde(default)]
    cs_return: Option<String>,
    #[serde(default)]
    cs_params: Vec<(String, String)>,
    #[serde(default)]
    cs_exceptions: Option<Vec<String>>,
}

const fn default_public() -> bool {
    true
}

impl From<WireOperation> for Operation {
    fn from(wire: WireOperation) -> Self {
        Self {
            name: wire.name.into(),
            module_path: wire.module_path.into(),
            fn_name: wire.fn_name.into(),
            is_setup: wire.is_setup,
            is_async: wire.is_async,
            is_method: wire.is_method,
            is_public: wire.is_public,
            return_type: wire.return_type.into(),
            fills: wire.fills.into(),
            params: wire.params.into_iter().map(Field::from).collect(),
            component: wire.component.into(),
            cs_class: wire.cs_class.map(Into::into),
            cs_method_of: wire.cs_method_of.map(Into::into),
            cs_method: wire.cs_method.map(Into::into),
            cs_is_static: wire.cs_is_static,
            cs_return: wire.cs_return.map(Into::into),
            cs_params: wire.cs_params.into_iter().map(Field::from).collect(),
            cs_exceptions: wire
                .cs_exceptions
                .map(|exceptions| exceptions.into_iter().map(Into::into).collect())
                .into(),
        }
    }
}

impl From<(String, String)> for Field {
    fn from((name, ty): (String, String)) -> Self {
        Self {
            name: name.into(),
            ty: ty.into(),
        }
    }
}

#[derive(Deserialize)]
struct WireVariant {
    name: String,
    #[serde(default)]
    fields: Vec<(String, String)>,
    #[serde(default)]
    tuple: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct WireType {
    name: String,
    kind: String,
    component: String,
    #[serde(default)]
    fields: Vec<(String, String)>,
    #[serde(default)]
    variants: Vec<WireVariant>,
}

impl From<WireType> for Type {
    fn from(wire: WireType) -> Self {
        Type::builder(TypeId::new(wire.component, wire.name, wire.kind))
            .fields(wire.fields.into_iter().map(Field::from).collect())
            .variants(
                wire.variants
                    .into_iter()
                    .map(|variant| Variant {
                        name: variant.name.into(),
                        fields: variant.fields.into_iter().map(Field::from).collect(),
                        tuple: variant.tuple.map(|items| items.into_iter().map(Into::into).collect()),
                    })
                    .collect(),
            )
            .build()
    }
}

impl Registry {
    /// Parse the runtime `discovery_json()` output into a registry.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed or missing required arrays.
    ///
    /// ```
    /// use specgate_discovery::registry::Registry;
    /// let registry = Registry::parse(r#"{"operations":[],"types":[]}"#)?;
    /// assert!(registry.present_components().is_empty());
    /// # Ok::<(), specgate_discovery::Error>(())
    /// ```
    pub fn parse(json: impl AsRef<str>) -> Result<Self, Error> {
        parse(json)
    }
}

#[cfg(test)]
mod tests {
    use super::{ExceptionMetadata, Field, Operation, OperationId, Type, TypeId, Variant};
    use crate::identity::{ComponentId, TypeName};

    #[test]
    fn identity_required() {
        let operation = Operation::builder(OperationId::new("demo.component", "health")).build();
        assert_eq!(operation.name, "health");
        assert_eq!(operation.component, "demo.component");
        assert!(operation.is_public);
    }

    #[test]
    fn parse_defaults() {
        let registry = super::Registry::parse(r#"{"operations":[{"name":"health","component":"demo"}],"types":[]}"#).unwrap();
        let operations = registry.operations_for("demo");
        assert_eq!(operations.len(), 1);
        assert!(operations[0].is_public);
    }

    #[test]
    fn registry_queries() {
        let registry = super::Registry::parse(
            r#"{
                "operations":[
                    {"name":"zeta","component":"demo"},
                    {"name":"alpha","component":"demo"},
                    {"name":"alpha","component":"demo","is_setup":true,"return_type":"State"}
                ],
                "types":[
                    {"name":"State","kind":"struct","component":"demo","fields":[["value","i32"]]},
                    {"name":"External","kind":"struct","component":"other"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            registry
                .operations_for("demo")
                .iter()
                .map(|operation| operation.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert_eq!(registry.local_types("demo")[0].fields[0].name, "value");
        assert_eq!(
            registry.present_components().iter().map(ComponentId::as_str).collect::<Vec<_>>(),
            ["demo", "other"]
        );
        assert_eq!(registry.setups_for("demo", "alpha").len(), 1);
        assert_eq!(registry.type_names(), ["External", "State"]);
    }

    #[test]
    fn visibility_override() {
        let operation = Operation::builder(OperationId::new("demo.component", "hidden"))
            .public(false)
            .build();
        assert!(!operation.is_public);
    }

    #[test]
    fn operation_builder_projects_all_metadata() {
        let operation = Operation::builder(("demo.component", "run"))
            .module_path("demo")
            .function("run_impl")
            .setup(true)
            .asynchronous(true)
            .method(true)
            .public(false)
            .return_type("Result<State, Fault>")
            .fills("state")
            .params(vec![Field {
                name: "state".into(),
                ty: "State".into(),
            }])
            .csharp_class(Some("Demo.Service".into()))
            .declaring_type(Some("Service".into()))
            .csharp_method(Some("Run".into()))
            .csharp_static(Some(true))
            .csharp_return(Some("State".into()))
            .csharp_params(vec![Field {
                name: "state".into(),
                ty: "State".into(),
            }])
            .csharp_exceptions(Some(vec![TypeName::from("Demo.Fault")]))
            .build();

        assert_eq!(operation.module_path, "demo");
        assert_eq!(operation.fn_name, "run_impl");
        assert!(operation.is_setup && operation.is_async && operation.is_method);
        assert!(!operation.is_public);
        assert_eq!(operation.return_type, "Result<State, Fault>");
        assert_eq!(operation.fills, "state");
        assert_eq!(operation.params.len(), 1);
        assert_eq!(operation.cs_class.as_deref(), Some("Demo.Service"));
        assert_eq!(operation.cs_method_of.as_deref(), Some("Service"));
        assert_eq!(operation.cs_method.as_deref(), Some("Run"));
        assert_eq!(operation.cs_is_static, Some(true));
        assert_eq!(operation.cs_return.as_deref(), Some("State"));
        assert_eq!(operation.cs_params.len(), 1);
        assert_eq!(operation.cs_exceptions.as_ref().unwrap(), [TypeName::from("Demo.Fault")]);
    }

    #[test]
    fn exception_metadata_preserves_protocol_states() {
        assert_eq!(ExceptionMetadata::from(None).as_ref(), None);
        assert_eq!(ExceptionMetadata::from(Some(Vec::new())).as_ref(), Some([].as_slice()));
        let named = ExceptionMetadata::from(Some(vec![TypeName::from("Demo.Fault")]));
        assert_eq!(named.as_ref().unwrap(), [TypeName::from("Demo.Fault")]);
    }

    #[test]
    fn type_builder() {
        let metadata = Type::builder(TypeId::new("demo.component", "Status", "enum"))
            .fields(vec![Field {
                name: "code".into(),
                ty: "i32".into(),
            }])
            .variants(vec![Variant {
                name: "Ready".into(),
                fields: vec![Field {
                    name: "value".into(),
                    ty: "i32".into(),
                }],
                tuple: Some(vec!["String".into()]),
            }])
            .build();
        assert_eq!(metadata.name, "Status");
        assert_eq!(metadata.kind, "enum");
        assert_eq!(metadata.fields[0].name, "code");
        assert_eq!(metadata.variants[0].name, "Ready");
    }

    #[test]
    fn malformed_metadata_is_rejected() {
        let error = super::Registry::parse(r#"{"operations":[{"name":"health","component":"demo","params":[["only-name"]]}],"types":[]}"#)
            .unwrap_err();
        assert!(error.is_registry());
        assert!(error.to_string().contains("failed to parse discovery JSON"));
    }

    #[test]
    fn empty_registry_queries_are_stable() {
        let registry = super::Registry::parse(r#"{"operations":[],"types":[]}"#).unwrap();
        assert!(registry.operations_for("missing").is_empty());
        assert!(registry.setups_for("missing", "operation").is_empty());
        assert!(registry.local_types("missing").is_empty());
        assert!(registry.present_components().is_empty());
        assert!(registry.type_names().is_empty());
    }
}
