//! Native declaration comparison, separate from CTSC Strict trace equality.

use super::{SemanticPath, ValidationDiagnostic};
use crate::validation::{DocumentBytes, RegistrySet, TypeRef, registry};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// One native declaration difference at an escaped JSON Pointer.
///
/// An absent side is distinct from a present declaration value. Values are
/// derived signature trees, not rewritten registry artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignatureDifference {
    /// Declaration location in the derived signature tree.
    pub path: SemanticPath,
    /// Reference declaration value, or absence.
    pub reference: Option<Value>,
    /// Candidate declaration value, or absence.
    pub candidate: Option<Value>,
}

/// Library result of comparing two independently validated native registries.
///
/// This is not the versioned CLI report format and does not assess trace
/// behavior, apply mappings, or establish full API equivalence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegistryComparisonReport {
    /// Reference root registry path.
    pub reference: PathBuf,
    /// Candidate root registry path.
    pub candidate: PathBuf,
    /// True only when both sets validate and their covered signatures match.
    pub signatures_match: bool,
    /// Per-side registry validation failures.
    pub validation_failures: Vec<ValidationDiagnostic>,
    /// Deterministically ordered native declaration differences.
    pub differences: Vec<SignatureDifference>,
}

/// Compare native Registry declarations without filesystem access.
///
/// Each root resolves only against its own supplied imports. Artifact identity,
/// versions, descriptions, and extensions are not signature fields. Ordered
/// operation inputs and tuple items retain their order; named members align by
/// name. Named references retain their resolved component/name identity.
///
/// This API covers the Registry contract accepted by this build. It does not
/// change CTSC Strict trace comparison or implement the CLI/report integration.
///
/// # Examples
///
/// ```
/// use specgate_ctsc::comparison::compare_registry_documents;
/// use specgate_ctsc::validation::DocumentBytes;
/// use std::path::Path;
/// let root = DocumentBytes::new(Path::new("registry.json"), b"{}");
/// let report = compare_registry_documents(root, &[], root, &[]);
/// assert!(!report.validation_failures.is_empty());
/// ```
#[must_use]
pub fn compare_registry_documents(
    reference: DocumentBytes<'_>,
    reference_imports: &[DocumentBytes<'_>],
    candidate: DocumentBytes<'_>,
    candidate_imports: &[DocumentBytes<'_>],
) -> RegistryComparisonReport {
    let reference_set = registry::load_bytes(reference, reference_imports);
    let candidate_set = registry::load_bytes(candidate, candidate_imports);
    let mut validation_failures = Vec::with_capacity(reference_set.issues.len() + candidate_set.issues.len());
    for (side, issues) in [("reference", reference_set.issues), ("candidate", candidate_set.issues)] {
        validation_failures.extend(issues.into_iter().map(|issue| ValidationDiagnostic {
            location: format!("{side} registry:{}", issue.location).into(),
            message: issue.message.into(),
        }));
    }
    let mut differences = Vec::new();
    let signatures_match = if validation_failures.is_empty()
        && let (Some(reference), Some(candidate)) = (reference_set.value.as_ref(), candidate_set.value.as_ref())
    {
        compare_values(Some(&signature(reference)), Some(&signature(candidate)), "", &mut differences);
        differences.is_empty()
    } else {
        false
    };
    RegistryComparisonReport {
        reference: reference.path.to_path_buf(),
        candidate: candidate.path.to_path_buf(),
        signatures_match,
        validation_failures,
        differences,
    }
}

fn signature(registry: &RegistrySet) -> Value {
    let components = registry
        .components
        .iter()
        .map(|(id, resolved)| {
            let component = &resolved.component;
            let scope = id.as_str();
            let operations = component
                .operations
                .iter()
                .map(|operation| {
                    let inputs = operation
                        .inputs
                        .iter()
                        .map(|input| json!({"name": input.name, "type": signature_type(&input.value_type, scope)}))
                        .collect::<Vec<_>>();
                    let observations = named_values(
                        operation.observations.iter().map(|value| (value.name.as_str(), &value.value_type)),
                        scope,
                    );
                    let errors = operation
                        .outcomes
                        .errors
                        .iter()
                        .map(|error| {
                            let mut declaration = Map::new();
                            if let Some(ty) = &error.value_type {
                                declaration.insert("type".to_string(), signature_type(ty, scope));
                            }
                            (error.name.clone(), Value::Object(declaration))
                        })
                        .collect::<Map<_, _>>();
                    let mut outcomes = Map::from_iter([
                        ("empty".to_string(), json!(operation.outcomes.empty)),
                        ("errors".to_string(), Value::Object(errors)),
                    ]);
                    if let Some(result) = &operation.outcomes.result {
                        outcomes.insert("result".to_string(), signature_type(result, scope));
                    }
                    (
                        operation.name.clone(),
                        json!({"inputs": inputs, "observations": observations, "outcomes": outcomes}),
                    )
                })
                .collect::<Map<_, _>>();
            let types = component
                .types
                .iter()
                .map(|ty| (ty.name().to_string(), signature_type(&ty.as_type(), scope)))
                .collect::<Map<_, _>>();
            let dependencies = component
                .dependencies
                .iter()
                .map(|dependency| dependency.component_id.as_str())
                .collect::<BTreeSet<_>>();
            (
                scope.to_string(),
                json!({"dependencies": dependencies, "operations": operations, "types": types}),
            )
        })
        .collect::<Map<_, _>>();
    json!({"components": components})
}

fn named_values<'a>(values: impl Iterator<Item = (&'a str, &'a TypeRef)>, scope: &str) -> Value {
    Value::Object(values.map(|(name, ty)| (name.to_string(), signature_type(ty, scope))).collect())
}

fn signature_type(ty: &TypeRef, scope: &str) -> Value {
    match ty {
        TypeRef::Primitive { name } => json!({"kind": "primitive", "name": name}),
        TypeRef::Named { name, component_id, .. } => {
            // Registry identity is provenance; validation resolves the component/name identity.
            json!({"kind": "named", "name": name, "componentId": component_id.as_ref().map_or(scope, |id| id.as_str())})
        }
        TypeRef::Collection { items, ordered, unique } => {
            json!({"kind": "collection", "items": signature_type(items, scope), "ordered": ordered, "unique": unique})
        }
        TypeRef::Map { keys, values, ordered } => {
            json!({"kind": "map", "keys": signature_type(keys, scope), "values": signature_type(values, scope), "ordered": ordered})
        }
        TypeRef::Tuple { items } => {
            json!({"kind": "tuple", "items": items.iter().map(|item| signature_type(item, scope)).collect::<Vec<_>>()})
        }
        TypeRef::Record { fields } => {
            json!({"kind": "record", "fields": named_values(fields.iter().map(|field| (field.name.as_str(), &field.value_type)), scope)})
        }
        TypeRef::Optional { value } => json!({"kind": "optional", "value": signature_type(value, scope)}),
        TypeRef::TaggedUnion { variants } => {
            let variants = variants
                .iter()
                .map(|variant| {
                    let mut declaration = Map::new();
                    if let Some(payload) = &variant.payload {
                        declaration.insert("payload".to_string(), signature_type(payload, scope));
                    }
                    (variant.name.clone(), Value::Object(declaration))
                })
                .collect::<Map<_, _>>();
            json!({"kind": "tagged_union", "variants": variants})
        }
    }
}

fn compare_values(reference: Option<&Value>, candidate: Option<&Value>, path: &str, differences: &mut Vec<SignatureDifference>) {
    if reference == candidate {
        return;
    }
    match (reference, candidate) {
        (Some(Value::Object(reference)), Some(Value::Object(candidate))) => {
            for key in reference.keys().chain(candidate.keys()).collect::<BTreeSet<_>>() {
                let segment = key.replace('~', "~0").replace('/', "~1");
                compare_values(reference.get(key), candidate.get(key), &format!("{path}/{segment}"), differences);
            }
        }
        (Some(Value::Array(reference)), Some(Value::Array(candidate))) => {
            for index in 0..reference.len().max(candidate.len()) {
                compare_values(reference.get(index), candidate.get(index), &format!("{path}/{index}"), differences);
            }
        }
        _ => differences.push(SignatureDifference {
            path: path.into(),
            reference: reference.cloned(),
            candidate: candidate.cloned(),
        }),
    }
}
