//! Encoding normalized discovery schemas as canonical CTSC registries.

pub mod error;

use crate::replay::model::{RegistryInput, Type};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// CTSC Registry Core contract version serialized into every document; changing it breaks compatibility.
const CTSC_VERSION: &str = "0.2.0";
// Registry Core format discriminator serialized into every document.
const REGISTRY_FORMAT: &str = "ctsc.registry";
// Discovery setup payload extension from the SpecGate normalized-schema contract; changing it breaks setup compatibility.
const SETUP_EXTENSION: &str = "dev.specgate.setups";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Encoded CTSC registry output.
#[expect(
    clippy::exhaustive_structs,
    reason = "encoding results are an exhaustively serialized CTSC boundary DTO"
)]
pub struct Encoding {
    /// Number of declared operations.
    pub operation_count: i32,
    /// Number of declared named types.
    pub type_count: i32,
    /// Compact registry JSON document.
    pub registry_json: String,
}

/// Registry identifier preserved exactly as supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id(String);
impl Id {
    /// Construct without adding lexical restrictions.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl<T: Into<String>> From<T> for Id {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Registry version preserved exactly as supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(String);
impl Version {
    /// Construct without adding lexical restrictions.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl<T: Into<String>> From<T> for Version {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Discovery-schema JSON preserved for parsing and normalization at encoding time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema(String);
impl Schema {
    /// Preserve unvalidated schema JSON for parsing at the encoding boundary.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}
impl<T: Into<String>> From<T> for Schema {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}
impl From<&Schema> for Schema {
    fn from(value: &Schema) -> Self {
        value.clone()
    }
}
impl std::fmt::Display for Schema {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl AsRef<str> for Schema {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
impl AsRef<Schema> for Schema {
    fn as_ref(&self) -> &Schema {
        self
    }
}

/// Encode one normalized discovery schema as a canonical CTSC registry.
///
/// The identity strings are preserved exactly; this boundary intentionally
/// adds no lexical validation.
///
/// # Examples
/// ```
/// let schema = r#"{"component":"example","dependencies":[],"dependency_types":[],"operations":[],"types":[]}"#;
/// let schema = specgate_ctsc::registry::Schema::new(schema);
/// let encoded = specgate_ctsc::registry::encode("registry", "1", schema)?;
/// assert!(encoded.registry_json.contains("\"registryId\":\"registry\""));
/// # Ok::<(), specgate_ctsc::registry::error::Error>(())
/// ```
/// # Errors
/// Returns a structured error when JSON, declarations, or type references are invalid.
pub fn encode(id: impl Into<Id>, version: impl Into<Version>, schema: impl AsRef<Schema>) -> error::Result<Encoding> {
    encode_one(id.into().0, version.into().0, <Schema as AsRef<str>>::as_ref(schema.as_ref()))
}

/// Merge normalized schemas and encode one canonical CTSC registry.
///
/// Input order does not affect output: components and declarations are merged
/// by semantic identity and emitted in canonical order.
///
/// # Examples
/// ```
/// use specgate_ctsc::registry::{Schema, encode_many};
/// let schemas = [Schema::new(r#"{"component":"a","dependencies":[],"dependency_types":[],"operations":[],"types":[]}"#)];
/// let encoded = encode_many("registry", "1", schemas)?;
/// assert_eq!(encoded.operation_count, 0);
/// # Ok::<(), specgate_ctsc::registry::error::Error>(())
/// ```
///
/// # Errors
/// Returns a structured error when any schema or merged declaration is invalid.
pub fn encode_many(id: impl Into<Id>, version: impl Into<Version>, schemas: impl AsRef<[Schema]>) -> error::Result<Encoding> {
    encode_all(id.into().0, version.into().0, schemas.as_ref())
}

/// Encode one normalized, setup-folded `SpecGate` schema without panicking.
///
/// # Errors
///
/// Returns an error when the schema JSON is malformed, a named type
/// declaration is unsupported, or a type reference is malformed, unsupported,
/// or names a type absent from the schema.
fn parse_components(schema_json: impl AsRef<str>) -> normalized::error::Result<Vec<Component>> {
    let mut components = Vec::new();
    append_components(schema_json, &mut components)?;
    Ok(components)
}

fn append_components(schema_json: impl AsRef<str>, components: &mut Vec<Component>) -> normalized::error::Result<()> {
    let schema: normalized::Schema = serde_json::from_str(schema_json.as_ref()).map_err(normalized::error::Error::malformed)?;
    let mut types_by_component = BTreeMap::new();
    insert_names(&mut types_by_component, &schema.component, &schema.types)?;
    for dependency in &schema.dependency_types {
        insert_names(&mut types_by_component, &dependency.component, &dependency.types)?;
    }
    let dependency_names = schema.dependencies.iter().cloned().collect::<BTreeSet<_>>();
    if dependency_names.len() != schema.dependencies.len() {
        return Err("normalized schema contains duplicate component dependencies".to_string().into());
    }
    let dependency_type_owners = schema
        .dependency_types
        .iter()
        .map(|dependency| dependency.component.as_ref())
        .collect::<BTreeSet<_>>();
    if dependency_type_owners.len() != schema.dependency_types.len() {
        return Err("normalized schema contains duplicate dependency type components".to_string().into());
    }
    if let Some(missing) = dependency_names
        .iter()
        .find(|dependency| !dependency_type_owners.contains(dependency.as_ref()))
    {
        return Err(format!("normalized schema dependency '{missing}' has no dependency type component").into());
    }
    let all_components = dependency_type_owners
        .iter()
        .copied()
        .chain(std::iter::once(schema.component.as_ref()))
        .collect::<BTreeSet<_>>();
    for dependency in &schema.dependency_types {
        if let Some(missing) = dependency
            .dependencies
            .iter()
            .find(|required| !all_components.contains(required.as_ref()))
        {
            return Err(format!(
                "normalized dependency component '{}' references missing dependency '{missing}'",
                dependency.component
            )
            .into());
        }
    }
    validate_cycles(&schema)?;
    let context = TypeContext {
        component: &schema.component,
        dependencies: &dependency_names,
        types_by_component: &types_by_component,
    };

    let mut operations = schema
        .operations
        .iter()
        .map(|operation| operation.to_ctsc(&context))
        .collect::<Result<Vec<_>, _>>()?;
    operations.sort_by(|left, right| left.name.cmp(&right.name));

    let mut types = schema.types.iter().map(|ty| ty.to_ctsc(&context)).collect::<Result<Vec<_>, _>>()?;
    types.sort_by(|left, right| left.name.cmp(&right.name));

    components.clear();
    components.reserve(schema.dependency_types.len() + 1);
    components.push(Component {
        id: schema.component.to_string(),
        dependencies: schema
            .dependencies
            .iter()
            .map(|component_id| ComponentRef {
                component_id: component_id.to_string(),
            })
            .collect(),
        operations,
        types,
    });
    let mut dependencies = schema.dependency_types.iter().collect::<Vec<_>>();
    dependencies.sort_by(|left, right| left.component.cmp(&right.component));
    let mut dependency_names = BTreeSet::new();
    for dependency in dependencies {
        dependency_names.clear();
        dependency_names.extend(dependency.dependencies.iter().cloned());
        let dependency_context = TypeContext {
            component: &dependency.component,
            dependencies: &dependency_names,
            types_by_component: &types_by_component,
        };
        let mut dependency_types = dependency
            .types
            .iter()
            .map(|ty| ty.to_ctsc(&dependency_context))
            .collect::<Result<Vec<_>, _>>()?;
        dependency_types.sort_by(|left, right| left.name.cmp(&right.name));
        components.push(Component {
            id: dependency.component.to_string(),
            dependencies: dependency
                .dependencies
                .iter()
                .map(|component_id| ComponentRef {
                    component_id: component_id.to_string(),
                })
                .collect(),
            operations: Vec::new(),
            types: dependency_types,
        });
    }
    Ok(())
}

fn encode_one(registry_id: String, registry_version: String, schema_json: impl AsRef<str>) -> error::Result<Encoding> {
    encode_document(registry_id, registry_version, parse_components(schema_json)?)
}

/// Encode several normalized component schemas as one deterministic registry.
///
/// Components contributed as dependency type closures are merged with their
/// full selected schema when both are present. Output is byte-identical to
/// [`encode_schema_registry_result`] for a single schema.
///
/// # Errors
///
/// Returns the same errors as [`encode_schema_registry_result`], or an error
/// when repeated component declarations disagree.
fn encode_all(registry_id: String, registry_version: String, schema_json: &[Schema]) -> error::Result<Encoding> {
    if schema_json.is_empty() {
        return Err("cannot encode a registry without normalized schemas".to_string().into());
    }
    let mut components = BTreeMap::<String, Component>::new();
    let mut decoded = Vec::new();
    for schema in schema_json {
        append_components(<Schema as AsRef<str>>::as_ref(schema), &mut decoded)?;
        for component in decoded.drain(..) {
            if let Some(existing) = components.get_mut(&component.id) {
                merge_component(existing, component)?;
            } else {
                components.insert(component.id.clone(), component);
            }
        }
    }
    encode_document(registry_id, registry_version, components.into_values().collect())
}

/// Serialize registry components as one deterministic CTSC registry document.
///
/// This is the single ordering rule for every registry this crate emits:
/// components are sorted by ascending component id, with no privileged
/// position for any selected or root component. `discover` and `capture`
/// therefore emit byte-identical documents for the same logical component set,
/// which matters because both write to the same `registry.ctsc.json` path.
fn encode_document(registry_id: String, registry_version: String, mut components: Vec<Component>) -> error::Result<Encoding> {
    components.sort_by(|left, right| left.id.cmp(&right.id));
    let operation_count = components.iter().try_fold(0_usize, |count, component| {
        count
            .checked_add(component.operations.len())
            .ok_or_else(|| error::Error::from("operation count overflow".to_string()))
    })?;
    let type_count = components.iter().try_fold(0_usize, |count, component| {
        count
            .checked_add(component.types.len())
            .ok_or_else(|| error::Error::from("type count overflow".to_string()))
    })?;
    let document = RegistryDocument {
        format: REGISTRY_FORMAT,
        format_version: CTSC_VERSION,
        registry_id,
        version: registry_version,
        components,
    };
    Ok(Encoding {
        operation_count: i32::try_from(operation_count).map_err(|_error| error::Error::from("operation count exceeds i32".to_string()))?,
        type_count: i32::try_from(type_count).map_err(|_error| error::Error::from("type count exceeds i32".to_string()))?,
        registry_json: {
            let mut json = serde_json::to_string(&document)?;
            json.shrink_to_fit();
            json
        },
    })
}

pub(crate) fn merge_component(existing: &mut Component, incoming: Component) -> error::Result<()> {
    let dependencies = existing
        .dependencies
        .iter()
        .chain(&incoming.dependencies)
        .map(|dependency| dependency.component_id.clone())
        .collect::<BTreeSet<_>>();
    existing.dependencies = dependencies
        .into_iter()
        .map(|component_id| ComponentRef {
            component_id: component_id.clone(),
        })
        .collect();

    for operation in incoming.operations {
        match existing.operations.iter().find(|candidate| candidate.name == operation.name) {
            Some(candidate) if candidate != &operation => {
                return Err(format!("normalized schemas disagree on operation '{}::{}'", existing.id, operation.name).into());
            }
            Some(_) => {}
            None => existing.operations.push(operation),
        }
    }
    existing.operations.sort_by(|left, right| left.name.cmp(&right.name));
    for ty in incoming.types {
        match existing.types.iter().find(|candidate| candidate.name == ty.name) {
            Some(candidate) if candidate != &ty => {
                return Err(format!("normalized schemas disagree on type '{}::{}'", existing.id, ty.name).into());
            }
            Some(_) => {}
            None => existing.types.push(ty),
        }
    }
    existing.types.sort_by(|left, right| left.name.cmp(&right.name));
    existing.operations.shrink_to_fit();
    existing.types.shrink_to_fit();
    Ok(())
}

mod normalized;
use normalized::{TypeContext, insert_names, validate_cycles};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryDocument {
    format: &'static str,
    format_version: &'static str,
    registry_id: String,
    version: String,
    components: Vec<Component>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Component {
    pub(crate) id: String,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) dependencies: Vec<ComponentRef>,
    pub(crate) operations: Vec<Operation>,
    pub(crate) types: Vec<NamedType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ComponentRef {
    pub(crate) component_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Operation {
    pub(crate) name: String,
    pub(crate) inputs: Vec<NamedValue>,
    pub(crate) observations: Vec<NamedValue>,
    pub(crate) outcomes: Outcomes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extensions: Option<BTreeMap<String, serde_json::Value>>,
}

type NamedValue = RegistryInput;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Outcomes {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) result: Option<TypeRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) empty: Option<bool>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub(crate) errors: Vec<ErrorOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ErrorOutcome {
    pub(crate) name: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub(crate) value_type: Option<TypeRef>,
}

type TypeRef = Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RegistryVariant {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<TypeRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NamedType {
    pub(crate) name: String,
    #[serde(flatten)]
    shape: TypeShape,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TypeShape {
    Record { fields: Vec<NamedValue> },
    TaggedUnion { variants: Vec<RegistryVariant> },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One component whose only dependency sorts strictly before it.
    fn dependency_first() -> String {
        serde_json::json!({
            "component": "zeta.app",
            "dependencies": ["alpha.core"],
            "dependency_types": [{
                "component": "alpha.core",
                "types": [{"name": "Widget", "kind": "struct", "fields": [{"name": "id", "ty": "i32"}]}]
            }],
            "operations": [{
                "name": "run",
                "is_async": false,
                "inputs": [{"name": "value", "ty": "i32"}],
                "output": "alpha.core::Widget"
            }],
            "types": []
        })
        .to_string()
    }

    fn component_ids(registry_json: impl AsRef<str>) -> Vec<String> {
        let document: serde_json::Value = serde_json::from_str(registry_json.as_ref()).expect("registry JSON");
        document["components"]
            .as_array()
            .expect("components array")
            .iter()
            .map(|component| component["id"].as_str().expect("component id").to_string())
            .collect()
    }

    fn component(json: serde_json::Value) -> Component {
        serde_json::from_value(json).expect("registry component")
    }

    fn operation(name: impl AsRef<str>, output: impl AsRef<str>) -> serde_json::Value {
        let name = name.as_ref();
        let output = output.as_ref();
        serde_json::json!({
            "name": name,
            "inputs": [],
            "observations": [],
            "outcomes": {"result": {"kind": "primitive", "name": output}}
        })
    }

    fn named_type(name: impl AsRef<str>, field: impl AsRef<str>) -> serde_json::Value {
        let name = name.as_ref();
        let field = field.as_ref();
        serde_json::json!({
            "name": name,
            "kind": "record",
            "fields": [{"name": field, "type": {"kind": "primitive", "name": "i32"}}]
        })
    }

    /// Components are ordered by id alone. The selected component gets no
    /// privileged first position, so a dependency that sorts before it leads.
    #[test]
    fn component_order() {
        let encoded = encode(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            Schema::new(dependency_first()),
        )
        .expect("schema encodes");
        assert_eq!(component_ids(&encoded.registry_json), vec!["alpha.core", "zeta.app"]);
        assert_eq!(encoded.operation_count, 1);
        assert_eq!(encoded.type_count, 1);
    }

    /// `discover` encodes one schema and `capture` encodes a set, but both
    /// write the same `registry.ctsc.json`. One ordering rule governs both, so
    /// the same logical component set must serialize to the same bytes.
    #[test]
    fn encoder_parity() {
        let schema = Schema::new(dependency_first());
        let single = encode("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &schema).expect("single encode");
        let many = encode_many(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            std::slice::from_ref(&schema),
        )
        .expect("multi encode");
        assert_eq!(single.registry_json.as_bytes(), many.registry_json.as_bytes());
        assert_eq!(single.operation_count, many.operation_count);
        assert_eq!(single.type_count, many.type_count);

        // Repeating a schema merges it into itself and must stay identical.
        let repeated = encode_many(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            &[schema.clone(), schema],
        )
        .expect("repeated encode");
        assert_eq!(repeated.registry_json.as_bytes(), single.registry_json.as_bytes());
        assert_eq!(repeated.operation_count, single.operation_count);
        assert_eq!(repeated.type_count, single.type_count);
    }

    /// The one shape where the merged document counts exceed every individual
    /// schema's: `alpha.core` contributes its own operations from its full
    /// schema *and* appears again as `zeta.app`'s dependency type closure.
    /// Merging must union the two views rather than let either replace the
    /// other, so the counts cover both components' whole surface.
    #[test]
    fn merged_counts() {
        let core = Schema::new(
            serde_json::json!({
                "component": "alpha.core",
                "operations": [{
                    "name": "build",
                    "is_async": false,
                    "inputs": [{"name": "id", "ty": "i32"}],
                    "output": "alpha.core::Widget"
                }],
                "types": [{"name": "Widget", "kind": "struct", "fields": [{"name": "id", "ty": "i32"}]}]
            })
            .to_string(),
        );
        let app = Schema::new(dependency_first());

        let merged = encode_many(
            "urn:ctsc:registry:zeta.app".to_string(),
            "0.1.0".to_string(),
            &[app.clone(), core.clone()],
        )
        .expect("merged encode");

        // `zeta.app::run` plus `alpha.core::build`; the dependency closure view
        // of `alpha.core` carries no operations and must not erase them.
        assert_eq!(merged.operation_count, 2);
        // `alpha.core::Widget` counted exactly once despite both views
        // declaring it.
        assert_eq!(merged.type_count, 1);
        assert_eq!(component_ids(&merged.registry_json), vec!["alpha.core", "zeta.app"]);

        // Both counts strictly exceed what either schema yields alone, which is
        // what the single-schema byte-equivalence test cannot reach.
        let app_only = encode("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &app).expect("app encode");
        assert_eq!((app_only.operation_count, app_only.type_count), (1, 1));
        let core_only = encode("urn:ctsc:registry:alpha.core".to_string(), "0.1.0".to_string(), &core).expect("core encode");
        assert_eq!((core_only.operation_count, core_only.type_count), (1, 1));
        assert!(merged.operation_count > app_only.operation_count && merged.operation_count > core_only.operation_count);

        // Schema order must not change the document.
        let reversed = encode_many("urn:ctsc:registry:zeta.app".to_string(), "0.1.0".to_string(), &[core, app]).expect("reversed encode");
        assert_eq!(reversed.registry_json.as_bytes(), merged.registry_json.as_bytes());
    }

    /// Repeated declarations of one component contribute the union of their
    /// dependencies, operations, and types, each kept in sorted order.
    #[test]
    fn deterministic_union() {
        let mut existing = component(serde_json::json!({
            "id": "zeta.app",
            "dependencies": [{"componentId": "beta.core"}],
            "operations": [operation("run", "i32")],
            "types": [named_type("Widget", "id")]
        }));
        let incoming = component(serde_json::json!({
            "id": "zeta.app",
            "dependencies": [{"componentId": "alpha.core"}, {"componentId": "beta.core"}],
            "operations": [operation("advance", "i32"), operation("run", "i32")],
            "types": [named_type("Gadget", "id"), named_type("Widget", "id")]
        }));
        merge_component(&mut existing, incoming).expect("compatible declarations merge");

        assert_eq!(
            existing
                .dependencies
                .iter()
                .map(|dependency| dependency.component_id.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha.core", "beta.core"],
            "dependencies are unioned and sorted"
        );
        assert_eq!(
            existing.operations.iter().map(|op| op.name.as_str()).collect::<Vec<_>>(),
            vec!["advance", "run"]
        );
        assert_eq!(
            existing.types.iter().map(|ty| ty.name.as_str()).collect::<Vec<_>>(),
            vec!["Gadget", "Widget"]
        );

        // Merging the same content again is a fixpoint: no duplicates, no
        // reordering, so declaration order cannot leak into the document.
        let again = existing.clone();
        let mut fixpoint = existing.clone();
        merge_component(&mut fixpoint, again).expect("idempotent merge");
        assert_eq!(fixpoint, existing);
    }

    /// Two declarations of the same operation or type name that disagree are
    /// reported rather than silently resolved to one of them.
    #[test]
    fn conflicting_merge() {
        let base = component(serde_json::json!({
            "id": "zeta.app",
            "operations": [operation("run", "i32")],
            "types": [named_type("Widget", "id")]
        }));

        let mut operations = base.clone();
        let error = merge_component(
            &mut operations,
            component(serde_json::json!({
                "id": "zeta.app",
                "operations": [operation("run", "i64")],
                "types": []
            })),
        )
        .expect_err("a divergent operation must be reported");
        assert_eq!(error.to_string(), "normalized schemas disagree on operation 'zeta.app::run'");

        let mut types = base;
        let error = merge_component(
            &mut types,
            component(serde_json::json!({
                "id": "zeta.app",
                "operations": [],
                "types": [named_type("Widget", "label")]
            })),
        )
        .expect_err("a divergent type must be reported");
        assert_eq!(error.to_string(), "normalized schemas disagree on type 'zeta.app::Widget'");
    }
}
