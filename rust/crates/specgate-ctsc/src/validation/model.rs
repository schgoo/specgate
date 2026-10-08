//! Lossless models for registry and trace documents under validation.
//!
//! [`RawText`] deliberately accepts every string because malformed document
//! values must survive parsing so later validation phases can report all
//! semantic issues with precise locations. Compact names and finite numeric
//! values are converted only after their corresponding checks succeed.

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::collections::BTreeMap;

#[path = "trace_model.rs"]
mod trace;
pub(crate) use trace::{AnyValue, CanonicalValue, F64Value, FiniteF64, SpanStatus, TraceDocument, TraceEvent, TraceSpan};

/// Unvalidated document text preserved verbatim so semantic checks can report its exact location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(transparent)]
pub(crate) struct RawText(Box<str>);
impl RawText {
    /// Borrow the original wire text without implying that semantic validation succeeded.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RawText {
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}
impl From<&str> for RawText {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl std::ops::Deref for RawText {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for RawText {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl std::borrow::Borrow<str> for RawText {
    fn borrow(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RawText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl PartialEq<str> for RawText {
    fn eq(&self, other: &str) -> bool {
        self.0.as_ref() == other
    }
}
impl PartialEq<&str> for RawText {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_ref() == *other
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Unvalidated span or event name retained for unsupported-name diagnostics.
pub(crate) struct RawName(Box<str>);
impl RawName {
    /// Borrow the wire name for protocol-name validation and location-aware diagnostics.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<String> for RawName {
    fn from(value: String) -> Self {
        Self(value.into_boxed_str())
    }
}
impl From<&str> for RawName {
    fn from(value: &str) -> Self {
        Self(value.into())
    }
}
impl std::ops::Deref for RawName {
    type Target = str;
    fn deref(&self) -> &str {
        &self.0
    }
}
impl AsRef<str> for RawName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Display for RawName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<&RawName> for String {
    fn from(value: &RawName) -> Self {
        value.0.to_string()
    }
}
impl PartialEq<str> for RawName {
    fn eq(&self, other: &str) -> bool {
        self.0.as_ref() == other
    }
}
impl PartialEq<&str> for RawName {
    fn eq(&self, other: &&str) -> bool {
        self.0.as_ref() == *other
    }
}
impl PartialEq<String> for RawName {
    fn eq(&self, other: &String) -> bool {
        self.0.as_ref() == other
    }
}

// These identifiers are defined by the CTSC Registry and Trace Core contracts.
// Producers and validators must update them together, which is why the version
// itself is read from the crate root rather than copied here: an encoded
// artifact and the validator that checks it cannot disagree about the version
// this build speaks.
pub(crate) use crate::CTSC_VERSION;
/// Closed set of semantic CTSC span names.
pub(crate) const CTSC_SPANS: [&str; 4] = [
    "conformance.run",
    "conformance.scenario",
    "conformance.operation",
    "conformance.parallel",
];
/// Closed set of semantic CTSC event names.
pub(crate) const CTSC_EVENTS: [&str; 6] = [
    "conformance.observation",
    "conformance.result",
    "conformance.empty",
    "conformance.error",
    "conformance.fault",
    "conformance.abandoned",
];

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional registry fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A parsed CTSC registry retaining all wire fields before shape and linkage checks.
pub(crate) struct RegistryDocument {
    /// Registry document kind discriminator.
    pub(crate) format: String,
    /// CTSC registry contract version.
    pub(crate) format_version: String,
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Semantic registry version used with its identity.
    pub(crate) version: RawText,
    #[serde(default)]
    /// Imported registry identities, digests, and retrieval URIs.
    pub(crate) imports: Vec<RegistryImport>,
    /// Component definitions owned by the registry set.
    pub(crate) components: Vec<RegistryComponent>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One unresolved registry import whose identity, digest, and URI are validated later.
pub(crate) struct RegistryImport {
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Semantic registry version used with its identity.
    pub(crate) version: RawText,
    /// Expected SHA-256 digest of the imported registry bytes.
    pub(crate) digest: RawText,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Templated retrieval location for the imported registry.
    pub(crate) uri: Option<String>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional component fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// One registry component retaining declaration order for deterministic-order checks.
pub(crate) struct RegistryComponent {
    /// Stable component identity within its owning registry.
    pub(crate) id: RawText,
    #[serde(default)]
    /// External component identities available to this component.
    pub(crate) dependencies: Vec<ComponentRef>,
    /// Semantic operations exposed by this component.
    pub(crate) operations: Vec<RegistryOperation>,
    /// Named semantic types exposed by this component.
    pub(crate) types: Vec<NamedType>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// A local or imported component reference awaiting registry linkage.
pub(crate) struct ComponentRef {
    /// Component identity referenced across registry boundaries.
    pub(crate) component_id: RawText,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: Option<RawText>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional operation fields not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// An operation declaration retaining raw outcome and semantic type references.
pub(crate) struct RegistryOperation {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    /// Declared semantic operation inputs in wire order.
    pub(crate) inputs: Vec<NamedValue>,
    /// Declared observations an operation may emit.
    pub(crate) observations: Vec<NamedValue>,
    /// Declared result, empty, and error completion channels.
    pub(crate) outcomes: RegistryOutcomes,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
    #[serde(default)]
    /// Protocol extension members preserved for extension validation.
    pub(crate) extensions: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// A named operation value paired with its unlinked semantic type.
pub(crate) struct NamedValue {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(rename = "type")]
    /// Semantic type of the named value.
    pub(crate) value_type: TypeRef,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// The result, empty-result marker, and declared errors for one operation.
pub(crate) struct RegistryOutcomes {
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Optional successful result channel type.
    pub(crate) result: Option<TypeRef>,
    #[serde(default)]
    /// Whether the operation permits explicit empty completion.
    pub(crate) empty: bool,
    #[serde(default)]
    /// Declared named error completion channels.
    pub(crate) errors: Vec<ErrorOutcome>,
}

#[expect(
    dead_code,
    reason = "strict deserialization validates optional error descriptions not otherwise inspected"
)]
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
/// A declared named error with an optional payload type.
pub(crate) struct ErrorOutcome {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(default, rename = "type", deserialize_with = "deserialize_present")]
    /// Semantic type of the named value.
    pub(crate) value_type: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[expect(dead_code, reason = "strict deserialization validates optional type descriptions and extensions")]
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// A named record or enum declaration in unlinked registry form.
pub(crate) enum NamedType {
    Record {
        name: String,
        fields: Vec<NamedValue>,
        #[serde(default, deserialize_with = "deserialize_present")]
        description: Option<String>,
        #[serde(default)]
        extensions: BTreeMap<String, Value>,
    },
    TaggedUnion {
        name: String,
        variants: Vec<Variant>,
        #[serde(default, deserialize_with = "deserialize_present")]
        description: Option<String>,
        #[serde(default)]
        extensions: BTreeMap<String, Value>,
    },
}

impl NamedType {
    /// Return the declaration name used to resolve references to this named type.
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::Record { name, .. } | Self::TaggedUnion { name, .. } => name,
        }
    }

    /// Project the declaration into the structural type used for value validation.
    pub(crate) fn as_type(&self) -> TypeRef {
        match self {
            Self::Record { fields, .. } => TypeRef::Record { fields: fields.clone() },
            Self::TaggedUnion { variants, .. } => TypeRef::TaggedUnion {
                variants: variants.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
/// One enum variant represented by record fields or a tuple payload.
pub(crate) struct Variant {
    /// Semantic protocol name interpreted according to the enclosing model.
    pub(crate) name: String,
    #[serde(default, deserialize_with = "deserialize_present")]
    /// Optional tuple or record payload carried by the variant.
    pub(crate) payload: Option<TypeRef>,
    #[serde(default, deserialize_with = "deserialize_present")]
    description: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
/// A recursive CTSC semantic type reference before named-type linkage.
pub(crate) enum TypeRef {
    Primitive {
        name: String,
    },
    Named {
        name: String,
        #[serde(default, rename = "componentId", deserialize_with = "deserialize_present")]
        component_id: Option<RawText>,
        #[serde(default, rename = "registryId", deserialize_with = "deserialize_present")]
        registry_id: Option<RawText>,
    },
    List {
        items: Box<TypeRef>,
    },
    Set {
        items: Box<TypeRef>,
    },
    Map {
        keys: Box<TypeRef>,
        values: Box<TypeRef>,
    },
    Tuple {
        items: Vec<TypeRef>,
    },
    Record {
        fields: Vec<NamedValue>,
    },
    Optional {
        value: Box<TypeRef>,
    },
    TaggedUnion {
        variants: Vec<Variant>,
    },
}

#[derive(Debug, Clone)]
/// The root registry and imported registries indexed for exact identity resolution.
pub(crate) struct RegistrySet {
    /// Identity of the registry being validated.
    pub(crate) root_id: RawText,
    /// Version of the registry being validated.
    pub(crate) root_version: RawText,
    /// Content digest of the registry being validated.
    pub(crate) root_digest: RawText,
    /// Component definitions owned by the registry set.
    pub(crate) components: BTreeMap<RawText, ResolvedComponent>,
}

#[derive(Debug, Clone)]
/// A linked component paired with the registry that owns its named types.
pub(crate) struct ResolvedComponent {
    /// Registry identity used for import and component linkage.
    pub(crate) registry_id: RawText,
    /// Linked component definition and its named-type scope.
    pub(crate) component: RegistryComponent,
}
