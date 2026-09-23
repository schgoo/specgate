//! CTSC implementation metadata discovery and semantic normalization.
//!
//! Rust targets self-report link-time operation/type metadata. C# targets are
//! built normally and reflected from their compiled assembly. Both retain raw
//! native invocation metadata and normalize the same semantic schema, including
//! deterministic setup folding.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

static DISCOVERY_ID: AtomicU64 = AtomicU64::new(0);

/// One black-box operation input, with any setup construction params folded in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredInput {
    pub name: String,
    /// Normalized spec type, e.g. `"i32"`, `"List<i32>"`, `"Option<i32>"`.
    pub ty: String,
}

/// One operation on the component's normalized surface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredOperation {
    pub name: String,
    pub is_async: bool,
    pub inputs: Vec<DiscoveredInput>,
    /// Normalized semantic return type; empty when the operation returns unit.
    pub output: String,
    /// Whether the operation may complete through CTSC's explicit empty channel.
    pub empty: bool,
    /// Declared semantic errors. Empty when discovery exposes no declaration.
    pub errors: Vec<DiscoveredError>,
    /// Setup producers folded into this operation's public input surface.
    pub setups: Vec<DiscoveredSetup>,
}

/// One declared operation error channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredError {
    pub name: String,
    pub ty: String,
}

/// One deterministic setup producer associated with an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredSetup {
    pub fills: String,
    pub inputs: Vec<DiscoveredInput>,
    pub output: String,
}

/// One named field (struct field or enum-variant field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredField {
    pub name: String,
    pub ty: String,
}

/// One enum variant. Named payloads populate `fields`; tuple payloads populate
/// `tuple`; unit variants populate neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredVariant {
    pub name: String,
    pub fields: Vec<DiscoveredField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tuple: Option<Vec<String>>,
}

/// One named complex type owned by the component. `kind` is `"struct"` or
/// `"enum"`; structs populate `fields`, enums populate `variants`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredType {
    pub name: String,
    pub kind: String,
    pub fields: Vec<DiscoveredField>,
    pub variants: Vec<DiscoveredVariant>,
}

/// A component's normalized, folded schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredSchema {
    pub component: String,
    pub dependencies: Vec<String>,
    pub dependency_types: Vec<DiscoveredDependencyTypes>,
    pub operations: Vec<DiscoveredOperation>,
    pub types: Vec<DiscoveredType>,
}

/// Named types owned by one dependency component and needed to resolve
/// component-qualified references in a standalone registry export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredDependencyTypes {
    pub component: String,
    pub dependencies: Vec<String>,
    pub types: Vec<DiscoveredType>,
}

/// Raw and normalized metadata for one selected binding target.
#[derive(Debug, Clone)]
pub struct TargetDiscovery {
    pub target: crate::binding::ResolvedTarget,
    pub cargo_context: Option<crate::support::CandidateCargoContext>,
    pub raw_registry_json: String,
    pub registry: Registry,
    pub schema: DiscoveredSchema,
}

/// Discover raw and normalized metadata for one target.
///
/// Rust targets use link-time registration and C# targets use reflection over
/// the compiled target assembly. Empty `component` is allowed for Rust callers
/// that need to select from the raw registry before normalization.
///
/// # Errors
///
/// Returns binding, target, build, reflection, registry, normalization, or
/// setup-ambiguity errors.
pub fn discover_target(binding_path: &str, target_name: Option<&str>, component: &str) -> Result<TargetDiscovery, String> {
    let target = crate::binding::resolve_binding_target(binding_path, target_name)?;
    discover_resolved_target(target, component)
}

/// Discover raw and normalized metadata from an already resolved target.
///
/// # Errors
///
/// Returns build, reflection, registry, normalization, or setup errors.
pub fn discover_resolved_target(target: crate::binding::ResolvedTarget, component: &str) -> Result<TargetDiscovery, String> {
    let requested: Vec<&str> = if component.is_empty() { Vec::new() } else { vec![component] };
    let mut many = discover_many_resolved_target(target, &requested)?;
    let schema = if component.is_empty() {
        DiscoveredSchema {
            component: String::new(),
            dependencies: Vec::new(),
            dependency_types: Vec::new(),
            operations: Vec::new(),
            types: Vec::new(),
        }
    } else {
        many.components
            .remove(component)
            .ok_or_else(|| format!("discovery returned no metadata for component '{component}'"))?
            .schema?
    };
    let raw_registry_json = many
        .raw_registry_json
        .into_iter()
        .next()
        .ok_or_else(|| "discovery produced no registry document".to_string())?;
    let registry = many
        .registries
        .into_iter()
        .next()
        .ok_or_else(|| "discovery produced no parsed registry".to_string())?;
    Ok(TargetDiscovery {
        target: many.target,
        cargo_context: many.cargo_context,
        raw_registry_json,
        registry,
        schema,
    })
}

/// Raw registry selection and normalized schema for one requested component.
#[derive(Debug, Clone)]
pub struct ComponentDiscovery {
    /// Index of this component's document in [`ManyTargetDiscovery::registries`].
    pub registry_index: usize,
    /// Normalized schema, or this component's exact normalization failure.
    pub schema: Result<DiscoveredSchema, String>,
}

/// Raw and normalized metadata for many components of one binding target.
///
/// Rust targets link and self-report once, so every component shares registry
/// index `0`. C# targets build and reflect once, emitting one raw document per
/// component. Either way, the expensive toolchain work happens a single time.
#[derive(Debug, Clone)]
pub struct ManyTargetDiscovery {
    pub target: crate::binding::ResolvedTarget,
    pub cargo_context: Option<crate::support::CandidateCargoContext>,
    /// Raw registry documents in emission order.
    pub raw_registry_json: Vec<String>,
    /// Parsed registries aligned with `raw_registry_json`.
    pub registries: Vec<Registry>,
    /// Requested components, sorted and deduplicated.
    pub components: BTreeMap<String, ComponentDiscovery>,
    /// Every component the compiled target declares an operation for, sorted
    /// and deduplicated, independent of what this run requested. Callers that
    /// must account for the whole target compare their expected coverage
    /// against this rather than against the requested subset.
    pub present_components: Vec<String>,
}

impl ManyTargetDiscovery {
    /// The parsed registry backing `component`.
    #[must_use]
    pub fn registry(&self, component: &str) -> Option<&Registry> {
        self.components
            .get(component)
            .and_then(|discovered| self.registries.get(discovered.registry_index))
    }

    /// The raw registry document backing `component`.
    #[must_use]
    pub fn raw_registry_json(&self, component: &str) -> Option<&str> {
        self.components
            .get(component)
            .and_then(|discovered| self.raw_registry_json.get(discovered.registry_index))
            .map(String::as_str)
    }

    /// The normalized schema result for `component`.
    #[must_use]
    pub fn schema(&self, component: &str) -> Option<&Result<DiscoveredSchema, String>> {
        self.components.get(component).map(|discovered| &discovered.schema)
    }
}

/// Discover raw and normalized metadata for many components of one target.
///
/// # Errors
///
/// Returns binding, target, build, reflection, or registry parse errors. A
/// component whose metadata cannot be normalized is reported in its own
/// [`ComponentDiscovery::schema`] rather than failing the whole batch.
pub fn discover_many_target(binding_path: &str, target_name: Option<&str>, components: &[&str]) -> Result<ManyTargetDiscovery, String> {
    let target = crate::binding::resolve_binding_target(binding_path, target_name)?;
    discover_many_resolved_target(target, components)
}

/// Discover raw and normalized metadata for many components of a resolved
/// target, paying the build/reflection cost once.
///
/// # Errors
///
/// Returns build, reflection, or registry parse errors.
pub fn discover_many_resolved_target(target: crate::binding::ResolvedTarget, components: &[&str]) -> Result<ManyTargetDiscovery, String> {
    let mut requested = components.iter().copied().filter(|name| !name.is_empty()).collect::<Vec<_>>();
    requested.sort_unstable();
    requested.dedup();
    let (raw_registry_json, cargo_context, shared_document, reported_components) = match target.language.as_str() {
        "rust" => {
            let context = crate::support::candidate_cargo_context(&target.target.package_root)?;
            (vec![run_discovery_with_context(&context)?], Some(context), true, None)
        }
        "csharp" => {
            if requested.is_empty() {
                return Err("C# discovery requires a non-empty component".to_string());
            }
            let discovered = crate::csharp_discovery::run_csharp_discovery_many(&target.target, &requested)?;
            (discovered.documents, None, false, Some(discovered.present_components))
        }
        other => return Err(format!("no discovery metadata emitted by {other} target")),
    };
    let registries = raw_registry_json
        .iter()
        .map(|json| Registry::parse(json))
        .collect::<Result<Vec<_>, String>>()?;
    // A shared Rust document already describes the whole linked target; a C#
    // run reflects one document per request, so its inventory comes from the
    // same single reflection pass instead.
    let present_components =
        reported_components.unwrap_or_else(|| registries.first().map(Registry::present_components).unwrap_or_default());
    let mut discovered = BTreeMap::new();
    for (position, component) in requested.iter().enumerate() {
        let registry_index = if shared_document { 0 } else { position };
        let registry = registries
            .get(registry_index)
            .ok_or_else(|| format!("discovery emitted no registry document for component '{component}'"))?;
        let schema = normalize_registry(registry, &target.language, component);
        discovered.insert((*component).to_string(), ComponentDiscovery { registry_index, schema });
    }
    Ok(ManyTargetDiscovery {
        target,
        cargo_context,
        raw_registry_json,
        registries,
        components: discovered,
        present_components,
    })
}

/// Discover only the raw registry JSON for one target.
///
/// # Errors
///
/// Returns the same errors as [`discover_target`].
pub fn discover_registry_json(binding_path: &str, target_name: Option<&str>, component: &str) -> Result<String, String> {
    Ok(discover_target(binding_path, target_name, component)?.raw_registry_json)
}

/// Discover one target's normalized, setup-folded component schema.
///
/// # Errors
///
/// Returns the same errors as [`discover_target`].
pub fn discover_target_schema(binding_path: &str, target_name: Option<&str>, component: &str) -> Result<DiscoveredSchema, String> {
    Ok(discover_target(binding_path, target_name, component)?.schema)
}

/// Normalize a previously parsed raw registry for one language and component.
///
/// # Errors
///
/// Returns semantic identity, visibility, setup, or malformed metadata errors.
pub fn normalize_registry(registry: &Registry, language: &str, component: &str) -> Result<DiscoveredSchema, String> {
    validate_component_surface(registry, component)?;
    let schema = if language == "csharp" {
        build_schema_prenormalized(registry, component)?
    } else {
        build_schema(registry, component)?
    };
    reject_dynamic_value_types(&schema)?;
    Ok(schema)
}

/// Reject component metadata that cannot describe a well-formed CTSC surface.
///
/// Discovery is the first place where a component's semantic identity is
/// known, so operation identity, visibility, and setup ownership are enforced
/// here rather than surfacing as confusing downstream encoding failures.
fn validate_component_surface(registry: &Registry, component: &str) -> Result<(), String> {
    let operations = registry.operations_for(component);
    let mut declarations: BTreeMap<&str, usize> = BTreeMap::new();
    for operation in &operations {
        *declarations.entry(operation.name.as_str()).or_insert(0) += 1;
    }
    if let Some((name, count)) = declarations.iter().find(|(_name, count)| **count > 1) {
        return Err(format!(
            "operation '{component}::{name}' is declared {count} times; operation identity must be unique within a component"
        ));
    }
    if let Some(operation) = operations.iter().find(|operation| !operation.is_public) {
        return Err(format!(
            "operation '{component}::{}' is declared on private function '{}'; discovery exposes only public operations",
            operation.name, operation.fn_name
        ));
    }
    let declared = operations.iter().map(|operation| operation.name.as_str()).collect::<BTreeSet<_>>();
    let mut orphans = registry
        .ops
        .iter()
        .filter(|candidate| candidate.is_setup && candidate.component == component && !declared.contains(candidate.name.as_str()))
        .collect::<Vec<_>>();
    orphans.sort_by(|left, right| left.name.cmp(&right.name).then_with(|| left.fn_name.cmp(&right.fn_name)));
    if let Some(orphan) = orphans.first() {
        return Err(format!(
            "setup '{}' for '{component}::{}' has no operation to construct; annotate the operation or remove the setup",
            orphan.fn_name, orphan.name
        ));
    }
    Ok(())
}

/// Reject a normalized surface that leaks the dynamic runtime value type.
///
/// A CTSC registry describes declared semantic types; the runtime's universal
/// `Value` has no registry encoding, so discovery rejects it with the exact
/// operation or type that introduced it.
fn reject_dynamic_value_types(schema: &DiscoveredSchema) -> Result<(), String> {
    let component = &schema.component;
    for operation in &schema.operations {
        for input in &operation.inputs {
            reject_dynamic_value(
                &input.ty,
                &format!("operation '{component}::{}' input '{}'", operation.name, input.name),
            )?;
        }
        reject_dynamic_value(&operation.output, &format!("operation '{component}::{}' output", operation.name))?;
        for error in &operation.errors {
            reject_dynamic_value(
                &error.ty,
                &format!("operation '{component}::{}' error '{}'", operation.name, error.name),
            )?;
        }
        for setup in &operation.setups {
            for input in &setup.inputs {
                reject_dynamic_value(
                    &input.ty,
                    &format!("setup for '{component}::{}' input '{}'", operation.name, input.name),
                )?;
            }
        }
    }
    for declared in schema
        .types
        .iter()
        .chain(schema.dependency_types.iter().flat_map(|owner| owner.types.iter()))
    {
        for field in &declared.fields {
            reject_dynamic_value(&field.ty, &format!("type '{}' field '{}'", declared.name, field.name))?;
        }
        for variant in &declared.variants {
            for field in &variant.fields {
                reject_dynamic_value(
                    &field.ty,
                    &format!("type '{}' variant '{}' field '{}'", declared.name, variant.name, field.name),
                )?;
            }
            for (position, element) in variant.tuple.iter().flatten().enumerate() {
                reject_dynamic_value(
                    element,
                    &format!("type '{}' variant '{}' element {position}", declared.name, variant.name),
                )?;
            }
        }
    }
    Ok(())
}

fn reject_dynamic_value(type_ref: &str, location: &str) -> Result<(), String> {
    let mentions_dynamic_value = type_ref
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .any(|token| token == "value");
    if mentions_dynamic_value {
        return Err(format!(
            "{location} type '{type_ref}' is the dynamic runtime value; CTSC registries require declared semantic types"
        ));
    }
    Ok(())
}

/// Build the canonical `DiscoveredSchema` for `comp` from a parsed registry:
/// operations (folded inputs, normalized output) sorted by name, plus the
/// component's own named types (fields/variants normalized) sorted by name.
fn build_schema(registry: &Registry, comp: &str) -> Result<DiscoveredSchema, String> {
    let type_names = registry.type_names();
    let graph = dependency_graph(registry, comp)?;
    let dependencies = graph.get(comp).cloned().unwrap_or_default();

    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| {
            let folded = fold_operation(op, registry)?;
            let mut outcome = operation_outcome(op, &type_names, false);
            outcome.output = qualify_type_reference(&outcome.output, registry, comp);
            for error in &mut outcome.errors {
                error.ty = qualify_type_reference(&error.ty, registry, comp);
            }
            Ok(DiscoveredOperation {
                name: op.name.clone(),
                is_async: op.is_async,
                inputs: folded
                    .inputs
                    .into_iter()
                    .map(|(name, ty)| DiscoveredInput {
                        name,
                        ty: map_type_for_component(&ty, &type_names, registry, comp),
                    })
                    .collect(),
                output: outcome.output,
                empty: outcome.empty,
                errors: outcome.errors,
                setups: folded
                    .setups
                    .into_iter()
                    .map(|setup| DiscoveredSetup {
                        fills: setup.fills,
                        inputs: setup
                            .inputs
                            .into_iter()
                            .map(|(name, ty)| DiscoveredInput {
                                name,
                                ty: map_type_for_component(&ty, &type_names, registry, comp),
                            })
                            .collect(),
                        output: map_type_for_component(&setup.output, &type_names, registry, comp),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| DiscoveredType {
            name: t.name.clone(),
            kind: t.kind.clone(),
            fields: t
                .fields
                .iter()
                .map(|(n, ty)| DiscoveredField {
                    name: n.clone(),
                    ty: map_type_for_component(ty, &type_names, registry, comp),
                })
                .collect(),
            variants: t
                .variants
                .iter()
                .map(|v| DiscoveredVariant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|(n, ty)| DiscoveredField {
                            name: n.clone(),
                            ty: map_type_for_component(ty, &type_names, registry, comp),
                        })
                        .collect(),
                    tuple: v.tuple.as_ref().map(|tuple| {
                        tuple
                            .iter()
                            .map(|ty| map_type_for_component(ty, &type_names, registry, comp))
                            .collect()
                    }),
                })
                .collect(),
        })
        .collect();

    Ok(DiscoveredSchema {
        component: comp.to_string(),
        dependency_types: dependency_types(registry, &graph, comp, false),
        dependencies,
        operations,
        types,
    })
}

/// Build a `DiscoveredSchema` from a registry whose type strings are ALREADY
/// normalized spec-type references (as emitted by the C# discovery program).
///
/// The setup-folding is language-neutral, so this reuses [`raw_inputs`] to fold
/// setup construction params into each operation's inputs (the invisible-setup
/// model). It differs from [`build_schema`] only in that it does NOT re-run the
/// Rust type mapper over the (already-normalized) type strings — it passes them
/// through verbatim, so the resulting schema is byte-identical to the Rust
/// canonical when the C# target conforms.
fn build_schema_prenormalized(registry: &Registry, comp: &str) -> Result<DiscoveredSchema, String> {
    let graph = dependency_graph(registry, comp)?;
    let dependencies = graph.get(comp).cloned().unwrap_or_default();
    let operations = registry
        .operations_for(comp)
        .into_iter()
        .map(|op| {
            let folded = fold_operation(op, registry)?;
            let mut outcome = operation_outcome(op, &[], true);
            outcome.output = qualify_type_reference(&outcome.output, registry, comp);
            for error in &mut outcome.errors {
                error.ty = qualify_type_reference(&error.ty, registry, comp);
            }
            Ok(DiscoveredOperation {
                name: op.name.clone(),
                is_async: op.is_async,
                inputs: folded
                    .inputs
                    .into_iter()
                    .map(|(name, ty)| DiscoveredInput {
                        name,
                        ty: qualify_type_reference(&ty, registry, comp),
                    })
                    .collect(),
                output: outcome.output,
                empty: outcome.empty,
                errors: outcome.errors,
                setups: folded
                    .setups
                    .into_iter()
                    .map(|setup| DiscoveredSetup {
                        fills: setup.fills,
                        inputs: setup
                            .inputs
                            .into_iter()
                            .map(|(name, ty)| DiscoveredInput {
                                name,
                                ty: qualify_type_reference(&ty, registry, comp),
                            })
                            .collect(),
                        output: qualify_type_reference(&setup.output, registry, comp),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let types = registry
        .local_types(comp)
        .into_iter()
        .map(|t| DiscoveredType {
            name: t.name.clone(),
            kind: t.kind.clone(),
            fields: t
                .fields
                .iter()
                .map(|(n, ty)| DiscoveredField {
                    name: n.clone(),
                    ty: qualify_type_reference(ty, registry, comp),
                })
                .collect(),
            variants: t
                .variants
                .iter()
                .map(|v| DiscoveredVariant {
                    name: v.name.clone(),
                    fields: v
                        .fields
                        .iter()
                        .map(|(n, ty)| DiscoveredField {
                            name: n.clone(),
                            ty: qualify_type_reference(ty, registry, comp),
                        })
                        .collect(),
                    tuple: v
                        .tuple
                        .as_ref()
                        .map(|tuple| tuple.iter().map(|ty| qualify_type_reference(ty, registry, comp)).collect()),
                })
                .collect(),
        })
        .collect();

    Ok(DiscoveredSchema {
        component: comp.to_string(),
        dependency_types: dependency_types(registry, &graph, comp, true),
        dependencies,
        operations,
        types,
    })
}

struct DiscoveredOutcome {
    output: String,
    empty: bool,
    errors: Vec<DiscoveredError>,
}

fn operation_outcome(op: &OpInfo, type_names: &[&str], prenormalized: bool) -> DiscoveredOutcome {
    if is_unit(&op.return_type) {
        return DiscoveredOutcome {
            output: String::new(),
            empty: false,
            errors: Vec::new(),
        };
    }
    let mapped = |ty: &str| {
        if prenormalized {
            normalize_type(ty)
        } else {
            map_type(ty, type_names).ref_string()
        }
    };
    if let Some(RustType::Named { name, args }) = parse_rust_type(&op.return_type)
        && name == "Option"
        && args.len() == 1
    {
        let value = rust_type_string(&args[0]);
        if is_unit(&value) {
            return DiscoveredOutcome {
                output: format!("Option<{}>", mapped(&value)),
                empty: false,
                errors: Vec::new(),
            };
        }
        return DiscoveredOutcome {
            output: mapped(&value),
            empty: true,
            errors: Vec::new(),
        };
    }
    if let Some(RustType::Named { name, args }) = parse_rust_type(&op.return_type)
        && name == "Result"
        && args.len() == 2
    {
        let ok = rust_type_string(&args[0]);
        let error = rust_type_string(&args[1]);
        return DiscoveredOutcome {
            output: if is_unit(&ok) { String::new() } else { mapped(&ok) },
            empty: false,
            errors: vec![DiscoveredError {
                name: "error".to_string(),
                ty: mapped(&error),
            }],
        };
    }
    DiscoveredOutcome {
        output: mapped(&op.return_type),
        empty: false,
        errors: Vec::new(),
    }
}

fn rust_type_string(ty: &RustType) -> String {
    match ty {
        RustType::Named { name, args } if args.is_empty() => name.clone(),
        RustType::Named { name, args } => format!("{name}<{}>", args.iter().map(rust_type_string).collect::<Vec<_>>().join(", ")),
        RustType::Ref(inner) => format!("&{}", rust_type_string(inner)),
        RustType::Slice(inner) => format!("[{}]", rust_type_string(inner)),
    }
}

fn dependency_graph(registry: &Registry, root: &str) -> Result<BTreeMap<String, Vec<String>>, String> {
    let mut graph = BTreeMap::new();
    let mut visiting = Vec::new();
    visit_component_dependencies(registry, root, true, &mut graph, &mut visiting)?;
    Ok(graph)
}

fn visit_component_dependencies(
    registry: &Registry,
    component: &str,
    include_operations: bool,
    graph: &mut BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
) -> Result<(), String> {
    if graph.contains_key(component) {
        return Ok(());
    }
    if let Some(position) = visiting.iter().position(|candidate| candidate == component) {
        let mut cycle = visiting[position..].to_vec();
        cycle.push(component.to_string());
        return Err(format!("component dependency cycle: {}", cycle.join(" -> ")));
    }
    visiting.push(component.to_string());
    let dependencies = direct_component_dependencies(registry, component, include_operations)?;
    for dependency in &dependencies {
        visit_component_dependencies(registry, dependency, false, graph, visiting)?;
    }
    visiting.pop();
    graph.insert(component.to_string(), dependencies);
    Ok(())
}

fn direct_component_dependencies(registry: &Registry, component: &str, include_operations: bool) -> Result<Vec<String>, String> {
    let mut references = Vec::new();
    if include_operations {
        let operation_names = registry
            .ops
            .iter()
            .filter(|operation| !operation.is_setup && operation.component == component)
            .map(|operation| operation.name.as_str())
            .collect::<BTreeSet<_>>();
        for operation in registry.ops.iter().filter(|operation| {
            operation.component == component && (!operation.is_setup || operation_names.contains(operation.name.as_str()))
        }) {
            for (_name, ty) in &operation.params {
                collect_named_refs(ty, &mut references);
            }
            collect_named_refs(&operation.return_type, &mut references);
        }
    }
    for ty in registry.types.iter().filter(|ty| ty.component == component) {
        for (_name, field_ty) in &ty.fields {
            collect_named_refs(field_ty, &mut references);
        }
        for variant in &ty.variants {
            for (_name, field_ty) in &variant.fields {
                collect_named_refs(field_ty, &mut references);
            }
            if let Some(tuple) = &variant.tuple {
                for field_ty in tuple {
                    collect_named_refs(field_ty, &mut references);
                }
            }
        }
    }
    let mut dependencies = BTreeSet::new();
    for name in references {
        if let Some(owner) = resolve_type_owner(registry, component, &name)?
            && owner != component
        {
            dependencies.insert(owner.to_string());
        }
    }
    Ok(dependencies.into_iter().collect())
}

fn resolve_type_owner<'a>(registry: &'a Registry, component: &str, name: &str) -> Result<Option<&'a str>, String> {
    if is_builtin_type(name) || name == "unit" {
        return Ok(None);
    }
    let local = registry
        .types
        .iter()
        .filter(|ty| ty.name == name && ty.component == component)
        .collect::<Vec<_>>();
    if local.len() > 1 {
        return Err(format!("component '{component}' contains duplicate type owner for '{name}'"));
    }
    if let Some(local) = local.first() {
        if local.component.is_empty() {
            return Err(format!("type '{name}' has no component owner"));
        }
        return Ok(Some(local.component.as_str()));
    }
    let external = registry.types.iter().filter(|ty| ty.name == name).collect::<Vec<_>>();
    match external.as_slice() {
        [] => Err(format!("referenced type '{name}' has no registered component owner")),
        [owner] if owner.component.is_empty() => Err(format!("type '{name}' has no component owner")),
        [owner] => Ok(Some(owner.component.as_str())),
        _ => Err(format!(
            "referenced type '{name}' has ambiguous component owners: {}",
            external
                .iter()
                .map(|ty| ty.component.as_str())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn map_type_for_component(ty: &str, type_names: &[&str], registry: &Registry, component: &str) -> String {
    qualify_type_reference(&map_type(ty, type_names).ref_string(), registry, component)
}

fn qualify_type_reference(type_ref: &str, registry: &Registry, component: &str) -> String {
    let owners = registry
        .types
        .iter()
        .filter(|ty| ty.component != component && !ty.component.is_empty())
        .map(|ty| (ty.name.as_str(), ty.component.as_str()))
        .collect::<BTreeMap<_, _>>();
    let characters = type_ref.char_indices().collect::<Vec<_>>();
    let mut output = String::with_capacity(type_ref.len());
    let mut position = 0;
    while position < characters.len() {
        let (start, character) = characters[position];
        if character.is_alphanumeric() || character == '_' {
            let mut end_position = position + 1;
            while end_position < characters.len() && (characters[end_position].1.is_alphanumeric() || characters[end_position].1 == '_') {
                end_position += 1;
            }
            let end = characters.get(end_position).map_or(type_ref.len(), |(index, _)| *index);
            let token = &type_ref[start..end];
            let already_qualified = type_ref[..start].ends_with("::");
            let is_local = registry.types.iter().any(|ty| ty.component == component && ty.name == token);
            if !already_qualified
                && !is_local
                && let Some(owner) = owners.get(token)
            {
                output.push_str(owner);
                output.push_str("::");
            }
            output.push_str(token);
            position = end_position;
        } else {
            output.push(character);
            position += 1;
        }
    }
    output
}

fn dependency_types(
    registry: &Registry,
    graph: &BTreeMap<String, Vec<String>>,
    root: &str,
    prenormalized: bool,
) -> Vec<DiscoveredDependencyTypes> {
    let type_names = registry.type_names();
    graph
        .iter()
        .filter(|(component, _dependencies)| component.as_str() != root)
        .map(|(component, dependencies)| DiscoveredDependencyTypes {
            component: component.clone(),
            dependencies: dependencies.clone(),
            types: registry
                .local_types(component)
                .into_iter()
                .map(|ty| DiscoveredType {
                    name: ty.name.clone(),
                    kind: ty.kind.clone(),
                    fields: ty
                        .fields
                        .iter()
                        .map(|(name, field_ty)| DiscoveredField {
                            name: name.clone(),
                            ty: if prenormalized {
                                qualify_type_reference(field_ty, registry, component)
                            } else {
                                map_type_for_component(field_ty, &type_names, registry, component)
                            },
                        })
                        .collect(),
                    variants: ty
                        .variants
                        .iter()
                        .map(|variant| DiscoveredVariant {
                            name: variant.name.clone(),
                            fields: variant
                                .fields
                                .iter()
                                .map(|(name, field_ty)| DiscoveredField {
                                    name: name.clone(),
                                    ty: if prenormalized {
                                        qualify_type_reference(field_ty, registry, component)
                                    } else {
                                        map_type_for_component(field_ty, &type_names, registry, component)
                                    },
                                })
                                .collect(),
                            tuple: variant.tuple.as_ref().map(|tuple| {
                                tuple
                                    .iter()
                                    .map(|ty| {
                                        if prenormalized {
                                            qualify_type_reference(ty, registry, component)
                                        } else {
                                            map_type_for_component(ty, &type_names, registry, component)
                                        }
                                    })
                                    .collect()
                            }),
                        })
                        .collect(),
                })
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Discovery build
// ---------------------------------------------------------------------------

/// Scaffold a temporary bin crate that links the target crate and prints its
/// `discovery_json()`, build+run it, and return the captured JSON.
///
/// # Errors
///
/// Returns an error string if the scaffold, build, or run fails, or if the
/// crate name cannot be read from `Cargo.toml`.
pub fn run_discovery(package_root: &Path) -> Result<String, String> {
    let context = crate::support::candidate_cargo_context(package_root)?;
    run_discovery_with_context(&context)
}

fn run_discovery_with_context(context: &crate::support::CandidateCargoContext) -> Result<String, String> {
    let invocation = DISCOVERY_ID.fetch_add(1, Ordering::Relaxed);
    let scratch = crate::support::InvocationCache::create("discovery", &context.package, invocation)?;
    std::fs::create_dir_all(scratch.path().join("src")).map_err(|e| format!("failed to scaffold discovery crate: {e}"))?;

    let cargo = discovery_cargo(context)?;
    std::fs::write(scratch.path().join("Cargo.toml"), cargo.manifest).map_err(|e| format!("failed to write discovery manifest: {e}"))?;
    let registry_config = cargo
        .config
        .map(|config| {
            let path = scratch.path().join("registry-config.toml");
            std::fs::write(&path, config).map_err(|error| format!("failed to write discovery registry config: {error}"))?;
            Ok::<_, String>(path)
        })
        .transpose()?;

    // `extern crate` forces the target crate's rlib to be linked so its
    // `linkme` registration statics (which are `#[used]`) are pulled in.
    let main_rs = "extern crate candidate;\nfn main() {\n    print!(\"{}\", specgate_runtime::discovery_json());\n}\n";
    std::fs::write(scratch.path().join("src").join("main.rs"), main_rs).map_err(|e| format!("failed to write discovery main.rs: {e}"))?;

    let mut cmd = Command::new(cargo_bin());
    cmd.arg("run").arg("--quiet");
    if let Some(config) = registry_config {
        cmd.arg("--config").arg(config);
    }
    cmd.arg("--manifest-path").arg(scratch.path().join("Cargo.toml"));
    cmd.current_dir(&context.path);
    cmd.env_remove("RUSTC_WORKSPACE_WRAPPER");
    cmd.env_remove("CARGO");
    cmd.env_remove("CARGO_MANIFEST_DIR");
    cmd.env("CARGO_TARGET_DIR", scratch.path().join("target").as_os_str());

    let output = cmd.output().map_err(|e| format!("failed to run discovery build: {e}"));
    let output = output?;
    if !output.status.success() {
        let error = format!("discovery build failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        return Err(error);
    }
    let json = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if json.is_empty() {
        return Err("discovery build produced no output".to_string());
    }
    Ok(json)
}

fn discovery_cargo(context: &crate::support::CandidateCargoContext) -> Result<crate::support::RunnerCargo, String> {
    let dependencies = BTreeMap::from([
        (
            "candidate".to_string(),
            crate::support::ManifestDependency::local(context.package.clone(), context.version.clone(), context.path.clone()),
        ),
        (
            "specgate_runtime".to_string(),
            crate::support::ManifestDependency::from_source(&context.runtime),
        ),
    ]);
    crate::support::runner_cargo("specgate-discovery-runner", dependencies)
}

#[must_use]
pub fn cargo_bin() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

// ---------------------------------------------------------------------------
// Registry model (parsed from discovery JSON)
// ---------------------------------------------------------------------------

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct OpInfo {
    pub name: String,
    pub module_path: String,
    pub fn_name: String,
    pub is_setup: bool,
    pub is_async: bool,
    pub is_method: bool,
    pub is_public: bool,
    pub return_type: String,
    pub fills: String,
    pub params: Vec<(String, String)>,
    pub component: String,
    /// C# declaring type, preserved exactly for future candidate replay.
    pub cs_class: Option<String>,
    /// C# declaring type's simple name.
    pub cs_method_of: Option<String>,
    /// C# method name.
    pub cs_method: Option<String>,
    /// Whether the C# method is static.
    pub cs_is_static: Option<bool>,
    /// Raw C# return signature, including nullable/generic spelling.
    pub cs_return: Option<String>,
    /// Raw C# parameter names and signatures.
    pub cs_params: Vec<(String, String)>,
    /// Declared C# exception type names; `None` means no exception metadata,
    /// while `Some([])` is the catch-all declaration.
    pub cs_exceptions: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct VariantInfo {
    pub name: String,
    pub fields: Vec<(String, String)>,
    pub tuple: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub name: String,
    pub kind: String,
    pub fields: Vec<(String, String)>,
    pub variants: Vec<VariantInfo>,
    pub component: String,
}

#[derive(Debug, Clone)]
pub struct Registry {
    pub ops: Vec<OpInfo>,
    pub types: Vec<TypeInfo>,
}

impl Registry {
    /// Parse the runtime `discovery_json()` output into a [`Registry`].
    ///
    /// # Errors
    ///
    /// Returns an error string if the JSON is malformed or missing the
    /// `operations`/`types` arrays.
    pub fn parse(json: &str) -> Result<Self, String> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("failed to parse discovery JSON: {e}"))?;
        let ops = v
            .get("operations")
            .and_then(serde_json::Value::as_array)
            .ok_or("discovery JSON missing 'operations' array")?
            .iter()
            .map(parse_op)
            .collect();
        let mut types: Vec<TypeInfo> = v
            .get("types")
            .and_then(serde_json::Value::as_array)
            .ok_or("discovery JSON missing 'types' array")?
            .iter()
            .map(parse_type)
            .collect();
        types.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Registry { ops, types })
    }

    /// Non-setup operations owned by `comp`, sorted by name.
    #[must_use]
    pub fn operations_for(&self, comp: &str) -> Vec<&OpInfo> {
        let mut ops: Vec<&OpInfo> = self.ops.iter().filter(|o| !o.is_setup && o.component == comp).collect();
        ops.sort_by(|a, b| a.name.cmp(&b.name));
        ops
    }

    /// Registered types owned by `comp`, sorted by name.
    #[must_use]
    pub fn local_types(&self, comp: &str) -> Vec<&TypeInfo> {
        let mut ts: Vec<&TypeInfo> = self.types.iter().filter(|t| t.component == comp).collect();
        ts.sort_by(|a, b| a.name.cmp(&b.name));
        ts
    }

    /// Distinct, sorted components present among non-setup operations and types.
    #[must_use]
    pub fn present_components(&self) -> Vec<String> {
        let mut set: BTreeSet<String> = BTreeSet::new();
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
    pub fn setups_for<'a>(&'a self, component: &str, op: &str) -> Vec<&'a OpInfo> {
        self.ops
            .iter()
            .filter(|candidate| candidate.is_setup && candidate.component == component && candidate.name == op)
            .collect()
    }

    /// The names of registered `SpecEvent` types.
    #[must_use]
    pub fn type_names(&self) -> Vec<&str> {
        self.types.iter().map(|t| t.name.as_str()).collect()
    }
}

fn parse_op(v: &serde_json::Value) -> OpInfo {
    OpInfo {
        name: str_field(v, "name"),
        module_path: str_field(v, "module_path"),
        fn_name: str_field(v, "fn_name"),
        is_setup: v.get("is_setup").and_then(serde_json::Value::as_bool).unwrap_or(false),
        is_async: v.get("is_async").and_then(serde_json::Value::as_bool).unwrap_or(false),
        is_method: v.get("is_method").and_then(serde_json::Value::as_bool).unwrap_or(false),
        is_public: v.get("is_public").and_then(serde_json::Value::as_bool).unwrap_or(true),
        return_type: str_field(v, "return_type"),
        fills: str_field(v, "fills"),
        params: parse_pairs(v.get("params")),
        component: str_field(v, "component"),
        cs_class: optional_str_field(v, "cs_class"),
        cs_method_of: optional_str_field(v, "cs_method_of"),
        cs_method: optional_str_field(v, "cs_method"),
        cs_is_static: v.get("cs_is_static").and_then(serde_json::Value::as_bool),
        cs_return: optional_str_field(v, "cs_return"),
        cs_params: parse_pairs(v.get("cs_params")),
        cs_exceptions: v.get("cs_exceptions").and_then(|value| {
            value
                .as_array()
                .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect())
        }),
    }
}

fn parse_type(v: &serde_json::Value) -> TypeInfo {
    let variants = v
        .get("variants")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|vv| VariantInfo {
                    name: str_field(vv, "name"),
                    fields: parse_pairs(vv.get("fields")),
                    tuple: vv
                        .get("tuple")
                        .and_then(serde_json::Value::as_array)
                        .map(|items| items.iter().filter_map(serde_json::Value::as_str).map(str::to_string).collect()),
                })
                .collect()
        })
        .unwrap_or_default();
    TypeInfo {
        name: str_field(v, "name"),
        kind: str_field(v, "kind"),
        fields: parse_pairs(v.get("fields")),
        variants,
        component: str_field(v, "component"),
    }
}

fn parse_pairs(v: Option<&serde_json::Value>) -> Vec<(String, String)> {
    v.and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    let pair = p.as_array()?;
                    Some((pair.first()?.as_str()?.to_string(), pair.get(1)?.as_str()?.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn str_field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_string()
}

fn optional_str_field(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(serde_json::Value::as_str).map(str::to_string)
}

// ---------------------------------------------------------------------------
// Type-ref mapping
// ---------------------------------------------------------------------------

/// A mapped spec type reference: either a scalar shorthand string or an inline
/// `map`/`set` object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecType {
    Scalar(String),
    Map { keys: Box<SpecType>, values: Box<SpecType> },
    Set { items: Box<SpecType> },
}

impl SpecType {
    /// Render as a single-line reference string (used inside shorthand wrappers
    /// such as `Option<…>` / `List<…>` / `Result<…>`).
    #[must_use]
    pub fn ref_string(&self) -> String {
        match self {
            SpecType::Scalar(s) => s.clone(),
            SpecType::Map { keys, values } => format!("map<{}, {}>", keys.ref_string(), values.ref_string()),
            SpecType::Set { items } => format!("set<{}>", items.ref_string()),
        }
    }
}

/// Parsed Rust type AST for semantic normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RustType {
    Named { name: String, args: Vec<RustType> },
    Ref(Box<RustType>),
    Slice(Box<RustType>),
}

/// Map a stringified Rust type (as produced by `quote!(#ty).to_string()`, which
/// inserts spaces around tokens) to its spec type reference, recursing into
/// generic arguments. `type_names` are the registered `SpecEvent` types (passed
/// through by bare name).
#[must_use]
pub fn map_type(ty: &str, type_names: &[&str]) -> SpecType {
    let parsed = parse_rust_type(ty).unwrap_or_else(|| RustType::Named {
        name: ty.trim().to_string(),
        args: Vec::new(),
    });
    map_rust_type(&parsed, type_names)
}

fn map_rust_type(t: &RustType, type_names: &[&str]) -> SpecType {
    match t {
        RustType::Ref(inner) => map_rust_type(inner, type_names),
        RustType::Slice(inner) => SpecType::Scalar(format!("List<{}>", map_rust_type(inner, type_names).ref_string())),
        RustType::Named { name, args } => map_named(name, args, type_names),
    }
}

fn map_named(name: &str, args: &[RustType], type_names: &[&str]) -> SpecType {
    let m = |t: &RustType| map_rust_type(t, type_names);
    match (name, args.len()) {
        ("String" | "str", _) => SpecType::Scalar("string".to_string()),
        ("()", 0) => SpecType::Scalar("unit".to_string()),
        // The runtime `Value` is the spec's built-in universal structured value.
        (_, 0) if is_runtime_value(name) => SpecType::Scalar("value".to_string()),
        ("Option", 1) => SpecType::Scalar(format!("Option<{}>", m(&args[0]).ref_string())),
        ("Vec", 1) => SpecType::Scalar(format!("List<{}>", m(&args[0]).ref_string())),
        ("Result", 2) => SpecType::Scalar(format!("Result<{}, {}>", m(&args[0]).ref_string(), m(&args[1]).ref_string())),
        ("HashMap" | "BTreeMap", 2) => SpecType::Map {
            keys: Box::new(m(&args[0])),
            values: Box::new(m(&args[1])),
        },
        ("HashSet" | "BTreeSet", 1) => SpecType::Set {
            items: Box::new(m(&args[0])),
        },
        // Named SpecEvent type or any other bare name → pass through by name.
        _ => SpecType::Scalar(name.to_string()),
    }
}

/// True for primitive scalars and the collection/option/result constructors
/// normalization maps directly, rather than as named `SpecEvent` references.
#[must_use]
pub fn is_builtin_type(name: &str) -> bool {
    is_runtime_value(name)
        || matches!(
            name,
            "String"
                | "string"
                | "str"
                | "char"
                | "bool"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "i128"
                | "isize"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "u128"
                | "usize"
                | "f32"
                | "f64"
                | "Option"
                | "optional"
                | "Vec"
                | "List"
                | "list"
                | "Result"
                | "HashMap"
                | "BTreeMap"
                | "Map"
                | "map"
                | "HashSet"
                | "BTreeSet"
                | "Set"
                | "set"
                | "()"
                | "unit"
                | "value"
        )
}

/// True for the runtime `specgate_runtime::Value` type in any of its stringified
/// forms. Type-path parsing keeps only the last path segment, so any of
/// `Value`, `specgate_runtime::Value`, `::specgate_runtime::Value`, or
/// `specgate::Value` arrive here as the bare name `Value`. It maps to the
/// built-in `value` spec type.
#[must_use]
pub fn is_runtime_value(name: &str) -> bool {
    matches!(
        name,
        "Value" | "specgate_runtime::Value" | "::specgate_runtime::Value" | "specgate::Value"
    )
}

/// Collect every non-builtin named type referenced inside a stringified Rust
/// type (recursing into generic args), in source order.
pub fn collect_named_refs(ty: &str, out: &mut Vec<String>) {
    if let Some(parsed) = parse_rust_type(ty) {
        collect_from_rust_type(&parsed, out);
    }
}

fn collect_from_rust_type(t: &RustType, out: &mut Vec<String>) {
    match t {
        RustType::Ref(inner) | RustType::Slice(inner) => collect_from_rust_type(inner, out),
        RustType::Named { name, args } => {
            if !is_builtin_type(name) {
                out.push(name.clone());
            }
            for a in args {
                collect_from_rust_type(a, out);
            }
        }
    }
}

/// Parse a (possibly space-separated) Rust type string into a [`RustType`].
fn parse_rust_type(s: &str) -> Option<RustType> {
    let tokens = tokenize_type(s);
    let mut pos = 0;
    let t = parse_type_tokens(&tokens, &mut pos)?;
    (pos == tokens.len()).then_some(t)
}

fn tokenize_type(s: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '<' | '>' | ',' | '&' | '[' | ']' | '(' | ')' => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
                tokens.push(c.to_string());
            }
            c if c.is_whitespace() => {
                if !cur.trim().is_empty() {
                    tokens.push(cur.trim().to_string());
                }
                cur.clear();
            }
            c => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        tokens.push(cur.trim().to_string());
    }
    tokens
}

fn parse_type_tokens(tokens: &[String], pos: &mut usize) -> Option<RustType> {
    let tok = tokens.get(*pos)?.as_str();
    match tok {
        "&" => {
            *pos += 1;
            // Skip a lifetime token (e.g. `'a`) if present.
            if tokens.get(*pos).is_some_and(|t| t.starts_with('\'')) {
                *pos += 1;
            }
            if tokens.get(*pos).map(String::as_str) == Some("mut") {
                *pos += 1;
            }
            let inner = parse_type_tokens(tokens, pos)?;
            Some(RustType::Ref(Box::new(inner)))
        }
        "[" => {
            *pos += 1;
            let inner = parse_type_tokens(tokens, pos)?;
            if tokens.get(*pos).map(String::as_str) != Some("]") {
                return None;
            }
            *pos += 1;
            Some(RustType::Slice(Box::new(inner)))
        }
        "(" => {
            if tokens.get(*pos + 1).map(String::as_str) != Some(")") {
                return None;
            }
            *pos += 2;
            Some(RustType::Named {
                name: "()".to_string(),
                args: Vec::new(),
            })
        }
        _ => {
            // Skip a leading path separator (`::Foo` / `::specgate_runtime::Value`).
            if tok == "::" {
                *pos += 1;
            }
            // A path like `std :: collections :: BTreeMap` — keep the last segment.
            let mut name = tokens.get(*pos)?.clone();
            *pos += 1;
            while tokens.get(*pos).map(String::as_str) == Some("::") {
                *pos += 1;
                if let Some(seg) = tokens.get(*pos) {
                    name.clone_from(seg);
                    *pos += 1;
                }
            }
            // Strip `::` if the tokenizer kept colons inside the segment.
            if let Some(idx) = name.rfind("::") {
                name = name[idx + 2..].to_string();
            }
            let mut args = Vec::new();
            if tokens.get(*pos).map(String::as_str) == Some("<") {
                *pos += 1;
                loop {
                    if *pos >= tokens.len() {
                        return None;
                    }
                    if tokens.get(*pos).map(String::as_str) == Some(">") {
                        return None;
                    }
                    let arg = parse_type_tokens(tokens, pos)?;
                    args.push(arg);
                    match tokens.get(*pos).map(String::as_str) {
                        Some(",") => {
                            *pos += 1;
                        }
                        Some(">") => {
                            *pos += 1;
                            break;
                        }
                        _ => return None,
                    }
                }
            }
            Some(RustType::Named { name, args })
        }
    }
}

/// Normalize a type string for equality comparison (collapse whitespace).
#[must_use]
pub fn normalize_type(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// True for the unit return type (`()` or empty).
#[must_use]
pub fn is_unit(ty: &str) -> bool {
    let n = normalize_type(ty);
    n.is_empty() || n == "()"
}

// ---------------------------------------------------------------------------
// Inputs (setup-aware)
// ---------------------------------------------------------------------------

struct FoldedSetup {
    fills: String,
    inputs: Vec<(String, String)>,
    output: String,
}

struct FoldedOperation {
    inputs: Vec<(String, String)>,
    setups: Vec<FoldedSetup>,
}

/// Fold exact component-scoped setup producers into one operation's public
/// input surface.
///
/// # Errors
///
/// Rejects missing `fills` parameters, ambiguous type-based fills, multiple
/// receiver producers, duplicate fills, and duplicate folded input names.
fn fold_operation(op: &OpInfo, registry: &Registry) -> Result<FoldedOperation, String> {
    let setups = registry.setups_for(&op.component, &op.name);
    let mut param_injection: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut receiver_setup: Option<&OpInfo> = None;
    let mut folded_setups = Vec::new();

    for setup in setups {
        let target = if setup.fills.is_empty() {
            let candidates = op
                .params
                .iter()
                .filter(|(name, ty)| !param_injection.contains_key(name) && normalize_type(ty) == normalize_type(&setup.return_type))
                .map(|(name, _ty)| name.clone())
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [] => None,
                [name] => Some(name.clone()),
                _ => {
                    return Err(format!(
                        "setup '{}' for '{}::{}' ambiguously matches parameters {} by return type '{}'; set fills explicitly",
                        setup.fn_name,
                        op.component,
                        op.name,
                        candidates.join(", "),
                        setup.return_type
                    ));
                }
            }
        } else {
            if !op.params.iter().any(|(name, _ty)| name == &setup.fills) {
                return Err(format!(
                    "setup '{}' for '{}::{}' fills unknown parameter '{}'",
                    setup.fn_name, op.component, op.name, setup.fills
                ));
            }
            Some(setup.fills.clone())
        };

        if let Some(target_name) = &target {
            if param_injection.insert(target_name.clone(), setup.params.clone()).is_some() {
                return Err(format!(
                    "multiple setups for '{}::{}' fill parameter '{target_name}'",
                    op.component, op.name
                ));
            }
        } else if receiver_setup.replace(setup).is_some() {
            return Err(format!(
                "multiple receiver setups are registered for '{}::{}'",
                op.component, op.name
            ));
        }
        folded_setups.push(FoldedSetup {
            fills: target.unwrap_or_default(),
            inputs: setup.params.clone(),
            output: setup.return_type.clone(),
        });
    }

    if op.is_method && receiver_setup.is_none() {
        return Err(format!(
            "operation '{}::{}' is a method with no receiver setup; annotate a #[spec_setup(\"{}\")] producer for its receiver",
            op.component, op.name, op.name
        ));
    }

    let mut inputs = Vec::new();
    if let Some(setup) = receiver_setup {
        inputs.extend(setup.params.iter().cloned());
    }
    for (name, ty) in &op.params {
        if let Some(injected) = param_injection.get(name) {
            inputs.extend(injected.iter().cloned());
        } else {
            inputs.push((name.clone(), ty.clone()));
        }
    }
    let mut names = BTreeSet::new();
    if let Some(duplicate) = inputs.iter().map(|(name, _ty)| name).find(|name| !names.insert((*name).clone())) {
        return Err(format!(
            "setup folding for '{}::{}' produces duplicate input name '{duplicate}'",
            op.component, op.name
        ));
    }
    Ok(FoldedOperation {
        inputs,
        setups: folded_setups,
    })
}

/// Build an operation's raw setup-folded inputs.
///
/// # Errors
///
/// Returns setup ambiguity and invalid-metadata errors.
pub fn raw_inputs(op: &OpInfo, registry: &Registry) -> Result<Vec<(String, String)>, String> {
    Ok(fold_operation(op, registry)?.inputs)
}

/// Build an operation's normalized setup-folded inputs.
///
/// # Errors
///
/// Returns setup ambiguity and invalid-metadata errors.
pub fn build_inputs(op: &OpInfo, registry: &Registry) -> Result<Vec<(String, SpecType)>, String> {
    let type_names = registry.type_names();
    Ok(raw_inputs(op, registry)?
        .into_iter()
        .map(|(name, ty)| (name, map_type(&ty, &type_names)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn registry(json: &str) -> Registry {
        Registry::parse(json).unwrap()
    }

    #[test]
    fn setup_folding_is_component_scoped() {
        let registry = registry(
            r#"{"operations":[
                {"name":"run","module_path":"fixture","fn_name":"run","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[["value","i32"]],"component":"component.a"},
                {"name":"run","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"State","fills":"","params":[["leaked","string"]],"component":"component.b"}
            ],"types":[]}"#,
        );
        assert_eq!(
            raw_inputs(&registry.ops[0], &registry).unwrap(),
            vec![("value".to_string(), "i32".to_string())]
        );
    }

    #[test]
    fn setup_folding_rejects_ambiguous_type_matches() {
        let registry = registry(
            r#"{"operations":[
                {"name":"run","module_path":"fixture","fn_name":"run","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[["left","State"],["right","State"]],"component":"component.a"},
                {"name":"run","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"State","fills":"","params":[],"component":"component.a"}
            ],"types":[]}"#,
        );
        assert!(raw_inputs(&registry.ops[0], &registry).unwrap_err().contains("ambiguously matches"));
    }

    #[test]
    fn semantic_surface_rejects_duplicate_private_and_orphan_declarations() {
        let duplicate = registry(
            r#"{"operations":[
                {"name":"render","module_path":"fixture","fn_name":"render_one","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.duplicate"},
                {"name":"render","module_path":"fixture","fn_name":"render_two","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.duplicate"}
            ],"types":[]}"#,
        );
        assert_eq!(
            normalize_registry(&duplicate, "rust", "fixture.duplicate").unwrap_err(),
            "operation 'fixture.duplicate::render' is declared 2 times; operation identity must be unique within a component"
        );

        let private = registry(
            r#"{"operations":[
                {"name":"secret","module_path":"fixture","fn_name":"secret","is_setup":false,"is_async":false,"is_method":false,"is_public":false,"return_type":"i32","fills":"","params":[],"component":"fixture.private"}
            ],"types":[]}"#,
        );
        assert_eq!(
            normalize_registry(&private, "rust", "fixture.private").unwrap_err(),
            "operation 'fixture.private::secret' is declared on private function 'secret'; discovery exposes only public operations"
        );

        let orphan = registry(
            r#"{"operations":[
                {"name":"increment","module_path":"fixture","fn_name":"make_counter","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[],"component":"fixture.orphan"}
            ],"types":[{"name":"Counter","module_path":"fixture","kind":"struct","component":"fixture.orphan","fields":[["count","i32"]],"variants":[]}]}"#,
        );
        assert_eq!(
            normalize_registry(&orphan, "rust", "fixture.orphan").unwrap_err(),
            "setup 'make_counter' for 'fixture.orphan::increment' has no operation to construct; annotate the operation or remove the setup"
        );
    }

    #[test]
    fn semantic_surface_rejects_unconstructible_methods_and_dynamic_values() {
        let method = registry(
            r#"{"operations":[
                {"name":"increment","module_path":"fixture","fn_name":"increment","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.method"}
            ],"types":[{"name":"Counter","module_path":"fixture","kind":"struct","component":"fixture.method","fields":[["count","i32"]],"variants":[]}]}"#,
        );
        assert_eq!(
            normalize_registry(&method, "rust", "fixture.method").unwrap_err(),
            "operation 'fixture.method::increment' is a method with no receiver setup; annotate a #[spec_setup(\"increment\")] producer for its receiver"
        );

        let dynamic = registry(
            r#"{"operations":[
                {"name":"echo","module_path":"fixture","fn_name":"echo","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"Value","fills":"","params":[["input","i32"]],"component":"fixture.value"}
            ],"types":[]}"#,
        );
        assert_eq!(
            normalize_registry(&dynamic, "rust", "fixture.value").unwrap_err(),
            "operation 'fixture.value::echo' output type 'value' is the dynamic runtime value; CTSC registries require declared semantic types"
        );

        let dynamic_field = registry(
            r#"{"operations":[
                {"name":"snapshot","module_path":"fixture","fn_name":"snapshot","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"Record","fills":"","params":[],"component":"fixture.value"}
            ],"types":[{"name":"Record","module_path":"fixture","kind":"struct","component":"fixture.value","fields":[["history","Vec<Value>"]],"variants":[]}]}"#,
        );
        assert_eq!(
            normalize_registry(&dynamic_field, "rust", "fixture.value").unwrap_err(),
            "type 'Record' field 'history' type 'List<value>' is the dynamic runtime value; CTSC registries require declared semantic types"
        );
    }

    #[test]
    fn raw_csharp_invocation_metadata_is_preserved() {
        let registry = registry(
            r#"{"operations":[{
                "name":"add","module_path":"","fn_name":"","is_setup":false,"is_async":true,"is_method":false,"is_public":true,
                "return_type":"i32","fills":"","params":[["a","i32"]],"component":"fixture.add",
                "cs_class":"Fixtures.Math","cs_method_of":"Math","cs_method":"Add","cs_is_static":true,
                "cs_return":"Task<int>","cs_params":[["a","int"]],"cs_exceptions":["InvalidOperationException"]
            }],"types":[]}"#,
        );
        let operation = &registry.ops[0];
        assert_eq!(operation.cs_class.as_deref(), Some("Fixtures.Math"));
        assert_eq!(operation.cs_method.as_deref(), Some("Add"));
        assert_eq!(operation.cs_return.as_deref(), Some("Task<int>"));
        assert_eq!(operation.cs_params, vec![("a".to_string(), "int".to_string())]);
        assert_eq!(
            operation.cs_exceptions.as_deref(),
            Some(["InvalidOperationException".to_string()].as_slice())
        );
    }

    #[test]
    fn nested_unit_types_parse_and_normalize() {
        assert_eq!(map_type("Option<()>", &[]).ref_string(), "Option<unit>");
        assert_eq!(map_type("Result<(), String>", &[]).ref_string(), "Result<unit, string>");

        let registry = registry(
            r#"{"operations":[
            {
                "name":"fallible","module_path":"fixture","fn_name":"fallible","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"Result<(), String>",
                "fills":"","params":[],"component":"fixture.unit"
            },
            {
                "name":"optional","module_path":"fixture","fn_name":"optional","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"Option<()>",
                "fills":"","params":[],"component":"fixture.unit"
            }],"types":[]}"#,
        );
        let schema = normalize_registry(&registry, "rust", "fixture.unit").unwrap();
        let fallible = schema.operations.iter().find(|operation| operation.name == "fallible").unwrap();
        assert!(fallible.output.is_empty());
        assert_eq!(fallible.errors[0].ty, "string");
        let optional = schema.operations.iter().find(|operation| operation.name == "optional").unwrap();
        assert_eq!(optional.output, "Option<unit>");
        assert!(!optional.empty);
    }

    #[test]
    fn rust_type_parser_rejects_trailing_or_unclosed_tokens() {
        assert!(parse_rust_type("Option<i32> trailing").is_none());
        assert!(parse_rust_type("Result<(), String").is_none());
        assert!(parse_rust_type("[i32").is_none());
    }

    #[test]
    fn dependency_owned_types_are_component_qualified() {
        let registry = registry(
            r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Shared"]],"component":"fixture.app"
            }],"types":[{
                "name":"Shared","module_path":"fixture","kind":"struct","component":"fixture.shared",
                "fields":[["id","i32"]],"variants":[]
            }]}"#,
        );
        let schema = normalize_registry(&registry, "rust", "fixture.app").unwrap();
        assert_eq!(schema.dependencies, vec!["fixture.shared"]);
        assert_eq!(schema.operations[0].inputs[0].ty, "fixture.shared::Shared");
        assert_eq!(schema.dependency_types[0].component, "fixture.shared");
        assert!(schema.dependency_types[0].dependencies.is_empty());
        assert_eq!(schema.dependency_types[0].types[0].name, "Shared");
    }

    #[test]
    fn dependency_closure_is_transitive_and_component_qualified() {
        let registry = registry(
            r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Middle"]],"component":"fixture.app"
            }],"types":[
                {"name":"Middle","module_path":"fixture","kind":"struct","component":"fixture.middle","fields":[["leaf","Leaf"]],"variants":[]},
                {"name":"Leaf","module_path":"fixture","kind":"struct","component":"fixture.leaf","fields":[["id","i32"]],"variants":[]}
            ]}"#,
        );
        let schema = normalize_registry(&registry, "rust", "fixture.app").unwrap();
        assert_eq!(schema.dependencies, vec!["fixture.middle"]);
        assert_eq!(schema.dependency_types.len(), 2);
        let middle = schema
            .dependency_types
            .iter()
            .find(|dependency| dependency.component == "fixture.middle")
            .unwrap();
        assert_eq!(middle.dependencies, vec!["fixture.leaf"]);
        assert_eq!(middle.types[0].fields[0].ty, "fixture.leaf::Leaf");
    }

    #[test]
    fn dependency_closure_rejects_cycles_and_missing_owners() {
        let cycle = registry(
            r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Middle"]],"component":"fixture.app"
            }],"types":[
                {"name":"AppType","module_path":"fixture","kind":"struct","component":"fixture.app","fields":[],"variants":[]},
                {"name":"Middle","module_path":"fixture","kind":"struct","component":"fixture.middle","fields":[["app","AppType"]],"variants":[]}
            ]}"#,
        );
        assert!(
            normalize_registry(&cycle, "rust", "fixture.app")
                .unwrap_err()
                .contains("fixture.app -> fixture.middle -> fixture.app")
        );

        let missing = registry(
            r#"{"operations":[{
                "name":"store","module_path":"fixture","fn_name":"store","is_setup":false,
                "is_async":false,"is_method":false,"is_public":true,"return_type":"()",
                "fills":"","params":[["value","Missing"]],"component":"fixture.app"
            }],"types":[]}"#,
        );
        assert!(normalize_registry(&missing, "rust", "fixture.app").unwrap_err().contains("Missing"));
    }

    #[test]
    fn packaged_discovery_manifest_uses_candidate_runtime_source() {
        let context = crate::support::CandidateCargoContext {
            package: "candidate".to_string(),
            version: "1.2.3".to_string(),
            path: PathBuf::from("candidate-package"),
            runtime: crate::support::CargoPackageSource {
                package: "specgate-runtime".to_string(),
                version: "0.6.0".to_string(),
                path: Some(PathBuf::from("resolved-runtime")),
                registry: None,
            },
        };
        let cargo = discovery_cargo(&context).unwrap();
        let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
        assert_eq!(parsed["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
        assert_eq!(
            parsed["dependencies"]["specgate_runtime"]["path"].as_str(),
            Some("resolved-runtime")
        );
        assert_eq!(parsed["dependencies"]["candidate"]["package"].as_str(), Some("candidate"));
        assert_eq!(cargo.config, None);
    }
}
