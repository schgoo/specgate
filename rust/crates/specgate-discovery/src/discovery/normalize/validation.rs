//! Semantic surface and dynamic-value validation.

use crate::discovery::model::Schema;
use crate::discovery::registry::Registry;
use crate::error::{Error, ErrorKind};
use crate::identity::{ComponentId, TypeExpression};
use std::collections::{BTreeMap, BTreeSet};

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Normalization, message)
}

/// Reject component metadata that cannot describe a well-formed CTSC surface.
///
/// Discovery is the first place where a component's semantic identity is
/// known, so operation identity, visibility, and setup ownership are enforced
/// here rather than surfacing as confusing downstream encoding failures.
pub(in crate::discovery) fn validate_surface(registry: &Registry, component: &ComponentId) -> Result<(), Error> {
    let component = component.as_str();
    let operations = registry.operations_for(component);
    let declarations = operations.iter().fold(BTreeMap::new(), |mut declarations, operation| {
        *declarations.entry(operation.name.as_str()).or_insert(0) += 1;
        declarations
    });
    if let Some((name, count)) = declarations.iter().find(|(_name, count)| **count > 1) {
        return Err(failure(format!(
            "operation '{component}::{name}' is declared {count} times; operation identity must be unique within a component"
        )));
    }
    if let Some(operation) = operations.iter().find(|operation| !operation.is_public) {
        return Err(failure(format!(
            "operation '{component}::{}' is declared on private function '{}'; discovery exposes only public operations",
            operation.name, operation.fn_name
        )));
    }
    let declared = operations.iter().map(|operation| operation.name.as_str()).collect::<BTreeSet<_>>();
    let orphan = registry
        .ops
        .iter()
        .filter(|candidate| candidate.is_setup && candidate.component == component && !declared.contains(candidate.name.as_str()))
        .min_by(|left, right| left.name.cmp(&right.name).then_with(|| left.fn_name.cmp(&right.fn_name)));
    if let Some(orphan) = orphan {
        return Err(failure(format!(
            "setup '{}' for '{component}::{}' has no operation to construct; annotate the operation or remove the setup",
            orphan.fn_name, orphan.name
        )));
    }
    Ok(())
}

/// Reject a normalized surface that leaks the dynamic runtime value type.
///
/// A CTSC registry describes declared semantic types; the runtime's universal
/// `Value` has no registry encoding, so discovery rejects it with the exact
/// operation or type that introduced it.
pub(in crate::discovery) fn reject_dynamic(schema: &Schema) -> Result<(), Error> {
    let component = &schema.component;
    for operation in &schema.operations {
        for input in &operation.inputs {
            reject_value(&input.ty, || {
                format!("operation '{component}::{}' input '{}'", operation.name, input.name)
            })?;
        }
        if let Some(output) = &operation.output {
            reject_value(output, || format!("operation '{component}::{}' output", operation.name))?;
        }
        for error in &operation.errors {
            if let Some(value_type) = &error.ty {
                reject_value(value_type, || {
                    format!("operation '{component}::{}' error '{}'", operation.name, error.name)
                })?;
            }
        }
        for setup in &operation.setups {
            for input in &setup.inputs {
                reject_value(&input.ty, || {
                    format!("setup for '{component}::{}' input '{}'", operation.name, input.name)
                })?;
            }
        }
    }
    for declared in schema
        .types
        .iter()
        .chain(schema.dependency_types.iter().flat_map(|owner| owner.types.iter()))
    {
        for field in &declared.fields {
            reject_value(&field.ty, || format!("type '{}' field '{}'", declared.name, field.name))?;
        }
        for variant in &declared.variants {
            for field in &variant.fields {
                reject_value(&field.ty, || {
                    format!("type '{}' variant '{}' field '{}'", declared.name, variant.name, field.name)
                })?;
            }
            for (position, element) in variant.tuple.iter().flatten().enumerate() {
                reject_value(element, || {
                    format!("type '{}' variant '{}' element {position}", declared.name, variant.name)
                })?;
            }
        }
    }
    Ok(())
}

/// Canonical schema token for an untyped runtime value, which CTSC declarations reject.
const DYNAMIC_TYPE: &str = "value";

fn reject_value(type_ref: &TypeExpression, location: impl FnOnce() -> String) -> Result<(), Error> {
    let type_ref = type_ref.as_str();
    let is_dynamic = type_ref
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|token| token == DYNAMIC_TYPE);
    if is_dynamic {
        let location = location();
        return Err(failure(format!(
            "{location} type '{type_ref}' is the dynamic runtime value; CTSC registries require declared semantic types"
        )));
    }
    Ok(())
}
