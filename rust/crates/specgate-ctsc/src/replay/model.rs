//! Strongly typed replay planning model.
//!
//! Model values are produced by [`super::decode`], while constructors validate
//! identities needed by callers building registry declarations.
//!
//! ```
//! use specgate_ctsc::replay::model::{RegistryInput, RegistryOp, OpDeps, Type};
//! let input = RegistryInput { name: "key".into(), value_type: Type::primitive("string") };
//! let duplicate = RegistryOp::builder(OpDeps {
//!     component_id: specgate_ctsc::replay::model::ComponentId::try_new("demo")?,
//!     name: specgate_ctsc::replay::model::OperationName::try_new("lookup")?,
//!     inputs: vec![input.clone(), input],
//! }).build();
//! assert!(duplicate.is_err());
//! # Ok::<(), specgate_ctsc::replay::Error>(())
//! ```

use super::error;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

macro_rules! guarded_text_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);
        impl $name {
            /// Construct a non-empty semantic identity.
            ///
            /// # Errors
            /// Returns [`Error`] when the spelling is empty, matching replay artifact validation.
            pub fn try_new(value: impl AsRef<str>) -> Result<Self, error::Error> {
                let value = value.as_ref();
                if value.is_empty() {
                    return Err(concat!(stringify!($name), " must not be empty").to_string().into());
                }
                Ok(Self(value.to_owned()))
            }
            /// Borrow the preserved spelling.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                Self::try_new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
            }
        }
        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
        impl PartialEq<String> for $name {
            fn eq(&self, other: &String) -> bool {
                self.as_str() == other
            }
        }
        impl PartialEq<&str> for $name {
            fn eq(&self, other: &&str) -> bool {
                self.as_str() == *other
            }
        }
    };
}
guarded_text_id!(RegistryId, "Registry identity used by replay linkage.");
guarded_text_id!(RegistryVersion, "Registry version used by replay linkage.");
/// Exact lowercase SHA-256 artifact digest used by replay linkage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ArtifactDigest(String);
// SHA-256 has 32 bytes, represented by exactly two hexadecimal characters each.
const SHA256_HEX_LEN: usize = 64;
impl ArtifactDigest {
    /// Parse `sha256:` followed by exactly 64 lowercase hexadecimal digits.
    ///
    /// # Errors
    /// Returns a replay decoding error when the digest is not canonical SHA-256 text.
    pub fn try_new(value: impl AsRef<str>) -> Result<Self, error::Error> {
        let value = value.as_ref();
        let valid = value.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == SHA256_HEX_LEN && hex.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        });
        if !valid {
            return Err("artifact digest must be 'sha256:' followed by 64 lowercase hexadecimal characters"
                .to_string()
                .into());
        }
        Ok(Self(value.to_owned()))
    }
    /// Borrow the canonical digest spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for ArtifactDigest {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::try_new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}
impl AsRef<str> for ArtifactDigest {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}
impl std::fmt::Display for ArtifactDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl PartialEq<String> for ArtifactDigest {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}
impl PartialEq<&str> for ArtifactDigest {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}
guarded_text_id!(ComponentId, "Component identity used by operation lookup.");
guarded_text_id!(OperationName, "Semantic operation name.");
guarded_text_id!(ScenarioName, "Semantic scenario name.");

/// Non-negative scenario ordering index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ScenarioIndex(i64);
impl ScenarioIndex {
    /// Construct a non-negative scenario index.
    #[must_use]
    pub fn new(value: i64) -> Option<Self> {
        (value >= 0).then_some(Self(value))
    }
    /// Return the numeric index.
    #[must_use]
    pub fn get(self) -> i64 {
        self.0
    }
}
/// Optional owner qualification for a named replay type.
///
/// This CTSC-owned boundary keeps identifier implementation types encapsulated
/// while preserving the normative `componentId` and `registryId` wire fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeOwner {
    #[serde(rename = "componentId", default, skip_serializing_if = "Option::is_none")]
    component_id: Option<ComponentId>,
    #[serde(rename = "registryId", default, skip_serializing_if = "Option::is_none")]
    registry_id: Option<RegistryId>,
}

impl TypeOwner {
    /// Build an optional named-type owner qualification from boundary text.
    ///
    /// # Errors
    /// Returns [`Error`](error::Error) when a supplied component or registry
    /// identity is empty.
    pub fn try_new<C: AsRef<str>, R: AsRef<str>>(component_id: Option<C>, registry_id: Option<R>) -> Result<Self, error::Error> {
        Ok(Self {
            component_id: component_id.map(|value| ComponentId::try_new(value.as_ref())).transpose()?,
            registry_id: registry_id.map(|value| RegistryId::try_new(value.as_ref())).transpose()?,
        })
    }

    pub(crate) fn from_ids(component_id: Option<ComponentId>, registry_id: Option<RegistryId>) -> Self {
        Self { component_id, registry_id }
    }

    /// Borrow the optional component qualification spelling.
    #[must_use]
    pub fn component_id(&self) -> Option<&str> {
        self.component_id.as_ref().map(ComponentId::as_str)
    }

    /// Borrow the optional registry qualification spelling.
    #[must_use]
    pub fn registry_id(&self) -> Option<&str> {
        self.registry_id.as_ref().map(RegistryId::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
/// Semantic type supported by replay planning.
pub struct Type(TypeKind);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TypeKind {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(flatten)]
        owner: TypeOwner,
    },
    List {
        items: Box<Type>,
    },
    Set {
        items: Box<Type>,
    },
    Map {
        keys: Box<Type>,
        values: Box<Type>,
    },
    Tuple {
        items: Vec<Type>,
    },
    Optional {
        value: Box<Type>,
    },
    Record {
        fields: Vec<RegistryInput>,
    },
}

impl Type {
    /// Construct a CTSC primitive type.
    #[must_use]
    pub fn primitive(name: impl Into<String>) -> Self {
        Self(TypeKind::Primitive { name: name.into() })
    }
    /// Construct a named type with optional ownership qualification.
    #[must_use]
    pub fn named(name: impl Into<String>, owner: TypeOwner) -> Self {
        Self(TypeKind::Named { name: name.into(), owner })
    }
    /// Construct an ordered list type.
    #[must_use]
    pub fn list(items: Type) -> Self {
        Self(TypeKind::List { items: Box::new(items) })
    }
    /// Construct an unordered set type.
    #[must_use]
    pub fn set(items: Type) -> Self {
        Self(TypeKind::Set { items: Box::new(items) })
    }
    /// Construct a key-value map type.
    #[must_use]
    pub fn map(keys: Type, values: Type) -> Self {
        Self(TypeKind::Map {
            keys: Box::new(keys),
            values: Box::new(values),
        })
    }
    /// Construct a fixed-position tuple type.
    #[must_use]
    pub fn tuple(items: impl Into<Vec<Type>>) -> Self {
        Self(TypeKind::Tuple { items: items.into() })
    }
    /// Construct an optional value type.
    #[must_use]
    pub fn optional(value: Type) -> Self {
        Self(TypeKind::Optional { value: Box::new(value) })
    }
    /// Construct an inline record type.
    #[must_use]
    pub fn record(fields: impl Into<Vec<RegistryInput>>) -> Self {
        Self(TypeKind::Record { fields: fields.into() })
    }
    pub(crate) fn kind_name(&self) -> &'static str {
        match &self.0 {
            TypeKind::Primitive { .. } => "primitive",
            TypeKind::Named { .. } => "named",
            TypeKind::List { .. } => "list",
            TypeKind::Set { .. } => "set",
            TypeKind::Map { .. } => "map",
            TypeKind::Tuple { .. } => "tuple",
            TypeKind::Optional { .. } => "optional",
            TypeKind::Record { .. } => "record",
        }
    }
    /// Return the primitive name when this is a primitive type.
    #[must_use]
    pub fn primitive_name(&self) -> Option<&str> {
        match &self.0 {
            TypeKind::Primitive { name } => Some(name),
            _ => None,
        }
    }
    /// Return the name and owner when this is a named type.
    #[must_use]
    pub fn named_parts(&self) -> Option<(&str, &TypeOwner)> {
        match &self.0 {
            TypeKind::Named { name, owner } => Some((name, owner)),
            _ => None,
        }
    }
    /// Return the element type for a list or set.
    #[must_use]
    pub fn element_type(&self) -> Option<&Type> {
        match &self.0 {
            TypeKind::List { items } | TypeKind::Set { items } => Some(items),
            _ => None,
        }
    }
    /// Return key and value types when this is a map.
    #[must_use]
    pub fn map_types(&self) -> Option<(&Type, &Type)> {
        match &self.0 {
            TypeKind::Map { keys, values } => Some((keys, values)),
            _ => None,
        }
    }
    /// Return ordered element types when this is a tuple.
    #[must_use]
    pub fn tuple_items(&self) -> Option<&[Type]> {
        match &self.0 {
            TypeKind::Tuple { items } => Some(items),
            _ => None,
        }
    }
    /// Return the wrapped type when this is optional.
    #[must_use]
    pub fn optional_value(&self) -> Option<&Type> {
        match &self.0 {
            TypeKind::Optional { value } => Some(value),
            _ => None,
        }
    }
    /// Return fields when this is an inline record.
    #[must_use]
    pub fn record_fields(&self) -> Option<&[RegistryInput]> {
        match &self.0 {
            TypeKind::Record { fields } => Some(fields),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One declared operation input.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct RegistryInput {
    /// Semantic input name.
    pub name: String,
    #[serde(rename = "type")]
    /// Semantic input type.
    pub value_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// One replayable registry operation.
pub struct RegistryOp {
    /// Owning component identifier.
    component_id: ComponentId,
    /// Semantic operation name.
    name: OperationName,
    /// Ordered operation inputs.
    inputs: Vec<RegistryInput>,
    /// Optional result type.
    output: Option<Type>,
    /// Whether empty completion is declared.
    empty: bool,
    /// Declared error outcomes.
    errors: Vec<Outcome>,
}

impl RegistryOp {
    /// Return the owning component identifier.
    #[must_use]
    pub fn component_id(&self) -> &ComponentId {
        &self.component_id
    }
    /// Return the semantic operation name.
    #[must_use]
    pub fn name(&self) -> &OperationName {
        &self.name
    }
    /// Return the ordered operation inputs.
    #[must_use]
    pub fn inputs(&self) -> &[RegistryInput] {
        &self.inputs
    }
    /// Return the optional result type.
    #[must_use]
    pub fn output(&self) -> Option<&Type> {
        self.output.as_ref()
    }
    /// Return whether empty completion is declared.
    #[must_use]
    pub fn empty(&self) -> bool {
        self.empty
    }
    /// Return the declared error outcomes.
    #[must_use]
    pub fn errors(&self) -> &[Outcome] {
        &self.errors
    }

    /// Start an operation declaration builder with required identity and inputs.
    pub fn builder(deps: impl Into<OpDeps>) -> RegistryOpBuilder {
        let deps = deps.into();
        RegistryOpBuilder {
            operation: Self {
                component_id: deps.component_id,
                name: deps.name,
                inputs: deps.inputs,
                output: None,
                empty: false,
                errors: Vec::new(),
            },
        }
    }
}

#[derive(Deserialize)]
struct RegistryOpWire {
    component_id: ComponentId,
    name: OperationName,
    inputs: Vec<RegistryInput>,
    output: Option<Type>,
    empty: bool,
    errors: Vec<Outcome>,
}

impl<'de> Deserialize<'de> for RegistryOp {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = RegistryOpWire::deserialize(deserializer)?;
        let mut builder = Self::builder(OpDeps {
            component_id: wire.component_id,
            name: wire.name,
            inputs: wire.inputs,
        });
        if let Some(output) = wire.output {
            builder = builder.output(output);
        }
        if wire.empty {
            builder = builder.empty();
        }
        for error in wire.errors {
            builder = builder.error(error);
        }
        builder.build().map_err(serde::de::Error::custom)
    }
}

/// Required dependencies for constructing a replay registry operation.
#[derive(Debug)]
#[expect(clippy::exhaustive_structs, reason = "builder dependencies support direct construction by callers")]
pub struct OpDeps {
    /// Owning component identifier.
    pub component_id: ComponentId,
    /// Semantic operation name.
    pub name: OperationName,
    /// Ordered operation inputs.
    pub inputs: Vec<RegistryInput>,
}

/// Builder for optional replay operation outcomes.
#[derive(Debug)]
pub struct RegistryOpBuilder {
    operation: RegistryOp,
}
impl RegistryOpBuilder {
    /// Declare a result type.
    #[must_use]
    pub fn output(mut self, output: Type) -> Self {
        self.operation.output = Some(output);
        self
    }
    /// Declare empty completion.
    #[must_use]
    pub fn empty(mut self) -> Self {
        self.operation.empty = true;
        self
    }
    /// Add a declared error outcome.
    #[must_use]
    pub fn error(mut self, error: Outcome) -> Self {
        self.operation.errors.push(error);
        self
    }
    /// Finish the declaration.
    ///
    /// # Errors
    /// Returns [`Error`] when input names or declared error names are duplicated.
    pub fn build(mut self) -> Result<RegistryOp, error::Error> {
        let unique_inputs = self
            .operation
            .inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<BTreeSet<_>>();
        if unique_inputs.len() != self.operation.inputs.len() {
            return Err("registry operation inputs must have unique names".to_string().into());
        }
        let unique_errors = self
            .operation
            .errors
            .iter()
            .map(|error| error.name.as_str())
            .collect::<BTreeSet<_>>();
        if unique_errors.len() != self.operation.errors.len() {
            return Err("registry operation errors must have unique names".to_string().into());
        }
        self.operation.inputs.shrink_to_fit();
        self.operation.errors.shrink_to_fit();
        Ok(self.operation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One declared replay error outcome.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Outcome {
    /// Error name.
    pub name: String,
    /// Optional error payload type.
    pub value_type: Option<Type>,
}

impl std::fmt::Display for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Registry linkage identity retained beside replay declarations.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct RegistryIdentity {
    /// Registry identifier.
    pub id: RegistryId,
    /// Registry version.
    pub version: RegistryVersion,
    /// Registry SHA-256 digest.
    pub digest: ArtifactDigest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Linked registry surface needed by replay.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Registry {
    /// Registry identity flattened to preserve the established replay wire shape.
    #[serde(flatten)]
    pub identity: RegistryIdentity,
    /// Replayable operation declarations.
    pub operations: Vec<RegistryOp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
/// Lossless primitive value supported by replay.
#[expect(
    clippy::exhaustive_enums,
    reason = "replay values are a closed CTSC primitive algebra exhaustively encoded by runners"
)]
pub enum Value {
    /// Unit value.
    Unit,
    /// UTF-8 string.
    String(String),
    /// Boolean.
    Bool(bool),
    /// Signed 32-bit integer.
    I32(i32),
    /// Signed 64-bit integer.
    I64(i64),
    /// Unsigned 32-bit integer.
    U32(u32),
    /// Unsigned 64-bit integer.
    U64(u64),
    /// Exact IEEE-754 binary32 bits.
    F32Bits(u32),
    /// Exact IEEE-754 binary64 bits.
    F64Bits(u64),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One replay operation input value.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Input {
    /// Semantic input name.
    pub name: String,
    /// Declared semantic type.
    pub value_type: Type,
    /// Decoded primitive value.
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One top-level operation selected for replay.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Operation {
    /// Owning component identifier.
    pub component_id: ComponentId,
    /// Semantic operation name.
    #[expect(clippy::struct_field_names, reason = "the replay protocol calls this operation_name")]
    pub operation_name: OperationName,
    /// Ordered semantic inputs.
    pub inputs: Vec<Input>,
    /// Optional result type.
    pub output: Option<Type>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// One ordered replay scenario.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Scenario {
    /// Scenario name.
    pub name: ScenarioName,
    /// Zero-based scenario order.
    pub index: ScenarioIndex,
    /// Ordered top-level operations.
    pub operations: Vec<Operation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
/// Verified capture data required to plan replay.
#[expect(
    clippy::exhaustive_structs,
    reason = "replay DTOs are an exhaustively serialized internal planning protocol"
)]
pub struct Bundle {
    /// Selected component identifier.
    pub component_id: ComponentId,
    /// Linked registry surface.
    pub registry: Registry,
    /// Ordered replay scenarios.
    pub scenarios: Vec<Scenario>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialization_rejects_duplicate_input_names() {
        let operation = RegistryOp::builder(OpDeps {
            component_id: ComponentId::try_new("example.component").unwrap(),
            name: OperationName::try_new("lookup").unwrap(),
            inputs: vec![RegistryInput {
                name: "key".to_string(),
                value_type: Type::primitive("string"),
            }],
        })
        .build()
        .unwrap();
        let mut document = serde_json::to_value(operation).unwrap();
        let duplicate = document["inputs"][0].clone();
        document["inputs"].as_array_mut().unwrap().push(duplicate);

        let error = serde_json::from_value::<RegistryOp>(document).unwrap_err();
        assert!(error.to_string().contains("unique names"));
    }

    #[test]
    fn guarded_identifiers_reject_empty_text() {
        ComponentId::try_new("").unwrap_err();
        OperationName::try_new("").unwrap_err();
        assert_eq!(ComponentId::try_new("component").unwrap().as_str(), "component");
    }

    #[test]
    fn artifact_digests_require_canonical_sha256() {
        let valid = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(ArtifactDigest::try_new(valid).unwrap().as_str(), valid);
        ArtifactDigest::try_new(format!("sha256:{}", "a".repeat(63))).unwrap_err();
        ArtifactDigest::try_new(format!("sha256:{}", "a".repeat(65))).unwrap_err();
        ArtifactDigest::try_new(format!("sha256:{}", "A".repeat(64))).unwrap_err();
    }

    #[test]
    fn builder_and_deserializer_preserve_valid_operations() {
        let operation = RegistryOp::builder(OpDeps {
            component_id: ComponentId::try_new("example.component").unwrap(),
            name: OperationName::try_new("lookup").unwrap(),
            inputs: Vec::new(),
        })
        .empty()
        .build()
        .unwrap();
        let encoded = serde_json::to_value(&operation).unwrap();
        assert_eq!(serde_json::from_value::<RegistryOp>(encoded).unwrap(), operation);
    }

    #[test]
    fn builder_rejects_duplicate_error_names() {
        let outcome = Outcome {
            name: "missing".to_string(),
            value_type: None,
        };
        let result = RegistryOp::builder(OpDeps {
            component_id: ComponentId::try_new("example.component").unwrap(),
            name: OperationName::try_new("lookup").unwrap(),
            inputs: Vec::new(),
        })
        .error(outcome.clone())
        .error(outcome)
        .build();
        assert!(result.unwrap_err().to_string().contains("unique names"));
    }

    #[test]
    fn scenario_indices_enforce_non_negative_boundaries() {
        assert_eq!(ScenarioIndex::new(0).unwrap().get(), 0);
        assert_eq!(ScenarioIndex::new(i64::MAX).unwrap().get(), i64::MAX);
        assert!(ScenarioIndex::new(-1).is_none());
    }
}
