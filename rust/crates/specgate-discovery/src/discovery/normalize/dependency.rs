//! Dependency closure, owner resolution, and type qualification.

use super::TypeSyntax;

use crate::discovery::model::{Dependency, Field, TypeDef, TypeDefDeps, TypeKind, Variant};
use crate::discovery::registry::Registry;
use crate::discovery::types::{builtin, collect_into, map};
use crate::error::{Error, ErrorKind};
use crate::identity::{ComponentId, TypeExpression, TypeName};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
enum Traversal {
    Root,
    Dependency,
}

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Normalization, message)
}

/// Build the component dependency closure rooted at `root`.
///
/// Returns a normalization error for cycles, missing owners, duplicate local
/// types, or ambiguous external owners.
pub(super) fn dependency_graph(root: impl AsRef<str>, registry: &Registry) -> Result<BTreeMap<ComponentId, Vec<ComponentId>>, Error> {
    let root = ComponentId::from(root.as_ref());
    let mut graph = BTreeMap::new();
    let mut visiting = Vec::new();
    visit(&root, Traversal::Root, &mut graph, &mut visiting, registry)?;
    Ok(graph)
}

fn visit(
    component: &ComponentId,
    traversal: Traversal,
    graph: &mut BTreeMap<ComponentId, Vec<ComponentId>>,
    visiting: &mut Vec<ComponentId>,
    registry: &Registry,
) -> Result<(), Error> {
    if graph.contains_key(component) {
        return Ok(());
    }
    if let Some(position) = visiting.iter().position(|candidate| candidate == component) {
        let mut cycle = visiting[position..].to_vec();
        cycle.push(component.clone());
        return Err(failure(format!(
            "component dependency cycle: {}",
            cycle.iter().map(ComponentId::as_str).collect::<Vec<_>>().join(" -> ")
        )));
    }
    visiting.push(component.clone());
    let dependencies = direct(component, traversal, registry)?;
    for dependency in &dependencies {
        visit(dependency, Traversal::Dependency, graph, visiting, registry)?;
    }
    visiting.pop();
    graph.insert(component.clone(), dependencies);
    Ok(())
}

fn direct(component: &ComponentId, traversal: Traversal, registry: &Registry) -> Result<Vec<ComponentId>, Error> {
    let mut references = Vec::new();
    if matches!(traversal, Traversal::Root) {
        let operation_names = registry
            .ops
            .iter()
            .filter(|operation| !operation.is_setup && operation.component == component.as_str())
            .map(|operation| operation.name.as_str())
            .collect::<BTreeSet<_>>();
        for operation in registry.ops.iter().filter(|operation| {
            operation.component == component.as_str() && (!operation.is_setup || operation_names.contains(operation.name.as_str()))
        }) {
            for parameter in &operation.params {
                collect_into(&parameter.ty, &mut references);
            }
            collect_into(&operation.return_type, &mut references);
        }
    }
    for ty in registry.types.iter().filter(|ty| ty.component == component.as_str()) {
        for field in &ty.fields {
            collect_into(&field.ty, &mut references);
        }
        for variant in &ty.variants {
            for field in &variant.fields {
                collect_into(&field.ty, &mut references);
            }
            if let Some(tuple) = &variant.tuple {
                for field_ty in tuple {
                    collect_into(field_ty, &mut references);
                }
            }
        }
    }
    let mut dependencies = BTreeSet::new();
    for name in references {
        if let Some(owner) = owner(&name, component, registry)?
            && owner.as_str() != component.as_str()
        {
            dependencies.insert(owner);
        }
    }
    Ok(dependencies.into_iter().collect())
}

/// Registry spelling for the CTSC unit primitive, which has no component owner.
const UNIT_TYPE: &str = "unit";

fn owner(name: impl AsRef<str>, component: &ComponentId, registry: &Registry) -> Result<Option<ComponentId>, Error> {
    let name = name.as_ref();
    if builtin(name) || name == UNIT_TYPE {
        return Ok(None);
    }
    let mut local = registry
        .types
        .iter()
        .filter(|ty| ty.name == name && ty.component == component.as_str());
    let first = local.next();
    if local.next().is_some() {
        return Err(failure(format!(
            "component '{component}' contains duplicate type owner for '{name}'"
        )));
    }
    if let Some(local) = first {
        if local.component.is_empty() {
            return Err(failure(format!("type '{name}' has no component owner")));
        }
        return Ok(Some(local.component.clone()));
    }

    let mut external = registry.types.iter().filter(|ty| ty.name == name);
    let Some(first) = external.next() else {
        return Err(failure(format!("referenced type '{name}' has no registered component owner")));
    };
    if external.next().is_some() {
        let owners = registry
            .types
            .iter()
            .filter(|ty| ty.name == name)
            .map(|ty| ty.component.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(failure(format!(
            "referenced type '{name}' has ambiguous component owners: {owners}"
        )));
    }
    if first.component.is_empty() {
        return Err(failure(format!("type '{name}' has no component owner")));
    }
    Ok(Some(first.component.clone()))
}

/// Reusable owner index for qualifying every type reference in one component.
///
/// Local names remain unqualified; uniquely owned external names receive their
/// component prefix. Construction preserves registry iteration semantics when
/// duplicate external names overwrite an earlier owner.
pub(super) struct Qualifier {
    external: BTreeMap<TypeName, ComponentId>,
    local: BTreeSet<TypeName>,
}

impl Qualifier {
    /// Index local and uniquely owned external types for one component.
    pub(super) fn new(component: impl AsRef<str>, registry: &Registry) -> Self {
        let component = component.as_ref();
        Self {
            external: registry
                .types
                .iter()
                .filter(|ty| ty.component != component && !ty.component.is_empty())
                .map(|ty| (ty.name.clone(), ty.component.clone()))
                .collect(),
            local: registry
                .types
                .iter()
                .filter(|ty| ty.component == component)
                .map(|ty| ty.name.clone())
                .collect(),
        }
    }

    /// Qualify external type tokens while preserving local and built-in names.
    pub(super) fn qualify(&self, type_ref: impl AsRef<str>) -> String {
        let type_ref = type_ref.as_ref();
        let mut characters = type_ref.char_indices().peekable();
        let mut output = String::with_capacity(type_ref.len());
        while let Some((start, character)) = characters.next() {
            if character.is_alphanumeric() || character == '_' {
                let mut end = start + character.len_utf8();
                while let Some(&(index, next)) = characters.peek() {
                    if !(next.is_alphanumeric() || next == '_') {
                        break;
                    }
                    characters.next();
                    end = index + next.len_utf8();
                }
                let token = &type_ref[start..end];
                let already_qualified = type_ref[..start].ends_with("::");
                if !already_qualified
                    && !self.local.contains(token)
                    && let Some(owner) = self.external.get(token)
                {
                    output.push_str(owner);
                    output.push_str("::");
                }
                output.push_str(token);
            } else {
                output.push(character);
            }
        }
        output
    }
}

/// Map one Rust type and qualify every component-owned semantic reference.
pub(super) fn map_component(ty: impl AsRef<str>, type_names: &[&str], qualifier: &Qualifier) -> Result<String, Error> {
    let mapped = map(ty.as_ref(), type_names)?;
    Ok(qualifier.qualify(mapped.ref_string()))
}

fn map_dependency(ty: impl AsRef<str>, syntax: TypeSyntax, type_names: &[&str], qualifier: &Qualifier) -> Result<TypeExpression, Error> {
    if matches!(syntax, TypeSyntax::Normalized) {
        Ok(qualifier.qualify(ty.as_ref()).into())
    } else {
        Ok(map_component(ty, type_names, qualifier)?.into())
    }
}

/// Collect declarations owned by dependencies in deterministic closure order.
///
/// Unresolved references retain their normalized schema spelling for the later
/// validation stage; collection itself is infallible.
pub(super) fn dependency_types(
    graph: &BTreeMap<ComponentId, Vec<ComponentId>>,
    root: impl AsRef<str>,
    syntax: TypeSyntax,
    registry: &Registry,
) -> Result<Vec<Dependency>, Error> {
    let root = root.as_ref();
    let type_names = registry.type_names();
    graph
        .iter()
        .filter(|(component, _dependencies)| component.as_str() != root)
        .map(|(component, dependencies)| {
            let qualifier = Qualifier::new(component, registry);
            let types = registry
                .local_types(component)
                .into_iter()
                .map(|ty| {
                    let fields = ty
                        .fields
                        .iter()
                        .map(|field| {
                            Ok(Field {
                                name: field.name.clone(),
                                ty: map_dependency(&field.ty, syntax, &type_names, &qualifier)?,
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    let variants = ty
                        .variants
                        .iter()
                        .map(|variant| {
                            let fields = variant
                                .fields
                                .iter()
                                .map(|field| {
                                    Ok(Field {
                                        name: field.name.clone(),
                                        ty: map_dependency(&field.ty, syntax, &type_names, &qualifier)?,
                                    })
                                })
                                .collect::<Result<Vec<_>, Error>>()?;
                            let tuple = variant
                                .tuple
                                .as_ref()
                                .map(|tuple| {
                                    tuple
                                        .iter()
                                        .map(|ty| map_dependency(ty, syntax, &type_names, &qualifier))
                                        .collect::<Result<Vec<_>, Error>>()
                                })
                                .transpose()?;
                            Ok(Variant {
                                name: variant.name.as_str().into(),
                                fields,
                                tuple,
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    TypeDef::builder(TypeDefDeps {
                        name: ty.name.as_str().into(),
                        kind: TypeKind::parse(ty.kind.as_str()),
                    })
                    .fields(fields)
                    .variants(variants)
                    .build()
                })
                .collect::<Result<Vec<_>, Error>>()?;
            Ok(Dependency::builder(component.as_str())
                .dependencies(dependencies.iter().map(|dependency| dependency.as_str().into()))
                .types(types)
                .build())
        })
        .collect()
}
