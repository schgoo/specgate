//! Rust and pre-normalized schema construction.
//!
//! Rust metadata is parsed and mapped from source-language type syntax, while
//! C# reflection metadata already carries normalized semantic spellings. Both
//! paths enforce the same component ownership, setup folding, dependency
//! qualification, deterministic ordering, unit-output, and error-channel
//! invariants before producing a language-neutral [`Schema`].

use super::TypeSyntax;
use super::dependency::{Qualifier, dependency_graph, dependency_types, map_component};
use crate::discovery::model::{ErrorDeclaration, Field, Input, Operation, Schema, Setup, TypeDef, TypeDefDeps, TypeKind, Variant};
use crate::discovery::registry::{Operation as RawOperation, Registry};
use crate::discovery::setup::fold_operation;
use crate::discovery::types::{RustType, is_unit, map, normalize, parse};
use crate::error::Error;

/// Validate all Rust type spellings once at the fallible schema boundary.
fn validate_syntax(registry: &Registry, type_names: &[&str]) -> Result<(), Error> {
    for operation in &registry.ops {
        map(&operation.return_type, type_names)?;
        for parameter in &operation.params {
            map(&parameter.ty, type_names)?;
        }
    }
    for declaration in &registry.types {
        for field in &declaration.fields {
            map(&field.ty, type_names)?;
        }
        for variant in &declaration.variants {
            for field in &variant.fields {
                map(&field.ty, type_names)?;
            }
            if let Some(tuple) = &variant.tuple {
                for ty in tuple {
                    map(ty, type_names)?;
                }
            }
        }
    }
    Ok(())
}

/// Build the canonical `Schema` for `comp` from a parsed registry:
/// operations (folded inputs, normalized output) sorted by name, plus the
/// component's own named types (fields/variants normalized) sorted by name.
pub(in crate::discovery) fn build_schema(registry: &Registry, comp: impl AsRef<str>) -> Result<Schema, Error> {
    let comp = comp.as_ref();
    let type_names = registry.type_names();
    validate_syntax(registry, &type_names)?;
    let graph = dependency_graph(comp, registry)?;
    let dependencies = graph.get(comp).cloned().unwrap_or_default();
    let qualifier = Qualifier::new(comp, registry);

    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| {
            let folded = fold_operation(op, registry)?;
            let mut outcome = operation_outcome(op, &type_names, TypeSyntax::Rust)?;
            outcome.output = outcome.output.map(|output| qualifier.qualify(&output));
            for error in &mut outcome.errors {
                error.ty = error.ty.take().map(|ty| qualifier.qualify(ty.as_str()).into());
            }
            Ok(Operation {
                name: op.name.clone(),
                is_async: op.is_async,
                inputs: folded
                    .inputs
                    .into_iter()
                    .map(|field| {
                        Ok(Input {
                            name: field.name,
                            ty: map_component(&field.ty, &type_names, &qualifier)?.into(),
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()?,
                output: outcome.output.map(Into::into),
                empty: outcome.empty,
                errors: outcome.errors,
                setups: folded
                    .setups
                    .into_iter()
                    .map(|setup| {
                        Ok(Setup {
                            fills: setup.fills,
                            inputs: setup
                                .inputs
                                .into_iter()
                                .map(|field| {
                                    Ok(Input {
                                        name: field.name,
                                        ty: map_component(&field.ty, &type_names, &qualifier)?.into(),
                                    })
                                })
                                .collect::<Result<Vec<_>, Error>>()?,
                            output: map_component(&setup.output, &type_names, &qualifier)?.into(),
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()?,
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| {
            let fields: Vec<Field> = t
                .fields
                .iter()
                .map(|field| {
                    Ok(Field {
                        name: field.name.clone(),
                        ty: map_component(&field.ty, &type_names, &qualifier)?.into(),
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            let variants: Vec<Variant> = t
                .variants
                .iter()
                .map(|v| {
                    let fields = v
                        .fields
                        .iter()
                        .map(|field| {
                            Ok(Field {
                                name: field.name.clone(),
                                ty: map_component(&field.ty, &type_names, &qualifier)?.into(),
                            })
                        })
                        .collect::<Result<Vec<_>, Error>>()?;
                    let tuple = v
                        .tuple
                        .as_ref()
                        .map(|tuple| {
                            tuple
                                .iter()
                                .map(|ty| Ok(map_component(ty, &type_names, &qualifier)?.into()))
                                .collect::<Result<Vec<_>, Error>>()
                        })
                        .transpose()?;
                    Ok(Variant {
                        name: v.name.clone(),
                        fields,
                        tuple,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?;
            TypeDef::builder(TypeDefDeps {
                name: t.name.clone(),
                kind: TypeKind::parse(t.kind.as_str()),
            })
            .fields(fields)
            .variants(variants)
            .build()
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(Schema {
        component: comp.into(),
        dependency_types: dependency_types(&graph, comp, TypeSyntax::Rust, registry)?,
        dependencies,
        operations,
        types,
    })
}

/// Build a `Schema` from a registry whose type strings are ALREADY
/// normalized spec-type references (as emitted by the C# discovery program).
///
/// The setup-folding is language-neutral, so this reuses [`raw_inputs`] to fold
/// setup construction params into each operation's inputs (the invisible-setup
/// model). It differs from [`build_schema`] only in that it does NOT re-run the
/// Rust type mapper over the (already-normalized) type strings — it passes them
/// through verbatim, so the resulting schema is byte-identical to the Rust
/// canonical when the C# target conforms.
pub(in crate::discovery) fn build_normalized(registry: &Registry, comp: impl AsRef<str>) -> Result<Schema, Error> {
    let comp = comp.as_ref();
    let graph = dependency_graph(comp, registry)?;
    let dependencies = graph.get(comp).cloned().unwrap_or_default();
    let qualifier = Qualifier::new(comp, registry);
    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| {
            let folded = fold_operation(op, registry)?;
            let mut outcome = operation_outcome(op, &[], TypeSyntax::Normalized)?;
            outcome.output = outcome.output.map(|output| qualifier.qualify(&output));
            for error in &mut outcome.errors {
                error.ty = error.ty.take().map(|ty| qualifier.qualify(ty.as_str()).into());
            }
            Ok(Operation {
                name: op.name.clone(),
                is_async: op.is_async,
                inputs: folded
                    .inputs
                    .into_iter()
                    .map(|field| Input {
                        name: field.name,
                        ty: qualifier.qualify(&field.ty).into(),
                    })
                    .collect(),
                output: outcome.output.map(Into::into),
                empty: outcome.empty,
                errors: outcome.errors,
                setups: folded
                    .setups
                    .into_iter()
                    .map(|setup| Setup {
                        fills: setup.fills,
                        inputs: setup
                            .inputs
                            .into_iter()
                            .map(|field| Input {
                                name: field.name,
                                ty: qualifier.qualify(&field.ty).into(),
                            })
                            .collect(),
                        output: qualifier.qualify(&setup.output).into(),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| {
            let fields: Vec<Field> = t
                .fields
                .iter()
                .map(|field| Field {
                    name: field.name.clone(),
                    ty: qualifier.qualify(&field.ty).into(),
                })
                .collect();
            let variants: Vec<Variant> = t
                .variants
                .iter()
                .map(|v| Variant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|field| Field {
                            name: field.name.clone(),
                            ty: qualifier.qualify(&field.ty).into(),
                        })
                        .collect(),
                    tuple: v
                        .tuple
                        .as_ref()
                        .map(|tuple| tuple.iter().map(|ty| qualifier.qualify(ty).into()).collect()),
                })
                .collect();
            TypeDef::builder(TypeDefDeps {
                name: t.name.clone(),
                kind: TypeKind::parse(t.kind.as_str()),
            })
            .fields(fields)
            .variants(variants)
            .build()
        })
        .collect::<Result<Vec<_>, Error>>()?;

    Ok(Schema {
        component: comp.into(),
        dependency_types: dependency_types(&graph, comp, TypeSyntax::Normalized, registry)?,
        dependencies,
        operations,
        types,
    })
}

struct DiscoveredOutcome {
    output: Option<String>,
    empty: bool,
    errors: Vec<ErrorDeclaration>,
}

fn operation_outcome(op: &RawOperation, type_names: &[&str], syntax: TypeSyntax) -> Result<DiscoveredOutcome, Error> {
    if is_unit(&op.return_type) {
        return Ok(DiscoveredOutcome {
            output: None,
            empty: false,
            errors: Vec::new(),
        });
    }
    let mapped = |ty: &str| -> Result<String, Error> {
        if matches!(syntax, TypeSyntax::Normalized) {
            Ok(normalize(ty))
        } else {
            Ok(map(ty, type_names)?.ref_string())
        }
    };
    if let Some(RustType::Named { name, args }) = parse(&op.return_type)
        && name == "Option"
        && args.len() == 1
    {
        let value = render(&args[0]);
        if is_unit(&value) {
            return Ok(DiscoveredOutcome {
                output: Some(format!("Option<{}>", mapped(&value)?)),
                empty: false,
                errors: Vec::new(),
            });
        }
        return Ok(DiscoveredOutcome {
            output: Some(mapped(&value)?),
            empty: true,
            errors: Vec::new(),
        });
    }
    if let Some(RustType::Named { name, args }) = parse(&op.return_type)
        && name == "Result"
        && args.len() == 2
    {
        let ok = render(&args[0]);
        let error = render(&args[1]);
        return Ok(DiscoveredOutcome {
            output: if is_unit(&ok) { None } else { Some(mapped(&ok)?) },
            empty: false,
            errors: vec![ErrorDeclaration {
                // CTSC represents Rust's single Result error channel with this
                // stable declaration name; changing it changes golden bytes.
                name: "error".into(),
                ty: Some(mapped(&error)?.into()),
            }],
        });
    }
    Ok(DiscoveredOutcome {
        output: Some(mapped(&op.return_type)?),
        empty: false,
        errors: Vec::new(),
    })
}

fn render(ty: &RustType) -> String {
    match ty {
        RustType::Named { name, args } if args.is_empty() => name.clone(),
        RustType::Named { name, args } => format!("{name}<{}>", args.iter().map(render).collect::<Vec<_>>().join(", ")),
        RustType::Ref(inner) => format!("&{}", render(inner)),
        RustType::Slice(inner) => format!("[{}]", render(inner)),
    }
}
