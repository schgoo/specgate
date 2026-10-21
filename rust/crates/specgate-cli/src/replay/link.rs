//! Static semantic linking from captured operations to candidates.
//!
//! Candidate discovery first resolves the requested component in the selected
//! binding. Planning then links each captured `(component, operation)` pair to
//! exactly one discovered operation, validates input and result compatibility,
//! and reuses links for repeated operations across scenarios. Registry entries
//! are trusted only after replay bundle validation, so an absent declaration is
//! an internal contract violation; candidate and type mismatches remain typed
//! replay errors. The resulting plan preserves scenario order while sharing its
//! immutable link table with execution.
//!
//! `specgate replay` calls `discover_candidate` followed by `build_plan`.
//! Both stages enforce candidate identity and type compatibility before any
//! invocation plan can reach execution.
use super::model::RegistryDigest;
use super::{
    BTreeSet, Candidate, Candidates, CommandEnvironment, Discovery, DiscoveryContext, DiscoveryInput, Input, Link, Operation, Path, Plan,
    PlannedOp, RawOperation, ReplayBundle, ReplayType, Scenario, Target,
};
use super::{Error, failure};

pub(super) fn discover_candidate(
    binding: impl AsRef<Path>,
    implementation: Option<&specgate_discovery::identity::ImplementationLabel>,
    component: impl AsRef<str>,
    discovery: &Discovery,
    system: &CommandEnvironment,
) -> Result<Candidate, Error> {
    let component = specgate_discovery::identity::ComponentId::from(component.as_ref());

    let binding = binding.as_ref();
    Candidates::discover_with(
        &DiscoveryInput {
            binding,
            implementation,
            components: std::slice::from_ref(&component),
        },
        &DiscoveryContext {
            system,
            discovery,
            #[cfg(any(test, feature = "test-util"))]
            discovered: None,
        },
    )?
    .component(component.as_str())
}
pub(super) fn build_plan(bundle: &ReplayBundle, candidate: &Candidate) -> Result<Plan, Error> {
    let mut links = Vec::with_capacity(bundle.registry.operations.len());
    let mut link_indexes = rustc_hash::FxHashMap::with_capacity_and_hasher(bundle.registry.operations.len(), rustc_hash::FxBuildHasher);
    let mut scenarios = Vec::with_capacity(bundle.scenarios.len());
    for scenario in &bundle.scenarios {
        let mut operations = Vec::with_capacity(scenario.operations.len());
        for operation in &scenario.operations {
            let key = (operation.component_id.as_str(), operation.operation_name.as_str());
            let link_index = if let Some(index) = link_indexes.get(&key) {
                *index
            } else {
                let declaration = bundle
                    .registry
                    .operations
                    .iter()
                    .find(|declaration| {
                        declaration.component_id() == &operation.component_id && declaration.name() == &operation.operation_name
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "verified replay operation '{}::{}' must exist in its registry",
                            operation.component_id, operation.operation_name
                        )
                    });
                let link = link_operation(declaration, candidate)?;
                let index = links.len();
                links.push(link);
                link_indexes.insert(key, index);
                index
            };
            operations.push(PlannedOp {
                link_index,
                inputs: operation.inputs.clone(),
            });
        }
        scenarios.push(Scenario {
            name: scenario.name.to_string(),
            index: u64::try_from(scenario.index.get())
                .map_err(|_conversion_error| format!("scenario index {} is negative", scenario.index.get()))?,
            operations,
        });
    }
    links.shrink_to_fit();
    validate_structured_types(bundle, candidate, &links)?;
    let mut test_target = None;
    for target in links
        .iter()
        .flat_map(|link| &link.setups)
        .filter_map(|setup| setup.test_target.as_ref())
    {
        if test_target.as_ref().is_some_and(|prior| prior != target) {
            return failure(
                "selected setup producers belong to different Cargo test targets; no single target contains the replay environment",
            );
        }
        test_target = Some(target.clone());
    }
    let plan = Plan {
        test_target,
        component_id: bundle.component_id.clone().as_str().into(),
        registry_id: bundle.registry.identity.id.as_str().to_string().into(),
        registry_version: bundle.registry.identity.version.as_str().to_string().into(),
        registry_digest: RegistryDigest::try_from(bundle.registry.identity.digest.as_str().to_string())?,
        registry_document: bundle.registry_document.clone(),
        structured_outcomes: bundle.registry.operations.iter().any(|operation| {
            operation.output().is_some_and(|ty| ty.primitive_name().is_none())
                || operation
                    .errors()
                    .iter()
                    .any(|error| error.value_type.as_ref().is_some_and(|ty| ty.primitive_name().is_none()))
        }),
        target: Target {
            crate_name: candidate.metadata().crate_name.clone(),
            language: candidate.metadata().language.into(),
            package_name: candidate.metadata().package_name.clone(),
            package_version: candidate.metadata().package_version.clone(),
            package_root: candidate.metadata().package_root.clone(),
            runtime: candidate.metadata().runtime.clone(),
        },
        links,
        scenarios,
    };
    serde_json::to_vec(&plan).map_err(|error| format!("failed to serialize target-local invocation plan: {error}"))?;
    Ok(plan)
}

pub(super) fn link_operation(reference: &specgate_ctsc::replay::model::RegistryOp, candidate: &Candidate) -> Result<Link, Error> {
    let raw_matches = candidate
        .metadata()
        .raw_registry
        .ops
        .iter()
        .filter(|operation| {
            !operation.is_setup
                && operation.component.as_str() == reference.component_id().as_str()
                && operation.name.as_str() == reference.name().as_str()
        })
        .collect::<Vec<_>>();
    let raw = match raw_matches.as_slice() {
        [] => {
            return failure(format!(
                "candidate is missing operation '{}::{}'",
                reference.component_id(),
                reference.name()
            ));
        }
        [operation] => *operation,
        operations => {
            return failure(format!(
                "candidate operation '{}::{}' is duplicated {} times",
                reference.component_id(),
                reference.name(),
                operations.len()
            ));
        }
    };
    if !raw.is_public {
        return failure(format!(
            "candidate operation '{}::{}' is not public",
            reference.component_id(),
            reference.name()
        ));
    }
    let normalized_matches = candidate
        .schema
        .operations
        .iter()
        .filter(|operation| operation.name.as_str() == reference.name().as_str())
        .collect::<Vec<_>>();
    let normalized = match normalized_matches.as_slice() {
        [] => {
            return failure(format!(
                "candidate normalized schema is missing operation '{}::{}'",
                reference.component_id(),
                reference.name()
            ));
        }
        [operation] => *operation,
        operations => {
            return failure(format!(
                "candidate normalized schema duplicates operation '{}::{}' {} times",
                reference.component_id(),
                reference.name(),
                operations.len()
            ));
        }
    };
    if normalized.is_async != raw.is_async {
        return failure(format!(
            "candidate operation '{}::{}' async metadata disagrees with normalized discovery",
            reference.component_id(),
            reference.name()
        ));
    }
    validate_inputs(reference, normalized)?;
    validate_output(reference, normalized)?;
    let folded_inputs = specgate_discovery::setup::raw_inputs(raw, &candidate.metadata().raw_registry)
        .map_err(|error| Error::wrap(super::ErrorKind::Linking, "candidate setup resolution failed", error))?;
    if folded_inputs.len() != normalized.inputs.len()
        || folded_inputs
            .iter()
            .zip(&normalized.inputs)
            .any(|(parameter, normalized_input)| parameter.name != normalized_input.name)
    {
        return failure(format!(
            "candidate raw metadata parameters for '{}::{}' do not match normalized input names/order",
            reference.component_id(),
            reference.name()
        ));
    }

    let operation_module = module_path(raw, &candidate.metadata().crate_name)?;
    validate_ident(&raw.fn_name, "candidate function name")?;
    let inputs = reference
        .inputs()
        .iter()
        .zip(&folded_inputs)
        .map(|(input, parameter)| {
            check_rust(
                &input.value_type,
                parameter.source_type.as_str(),
                ValueContext::new(reference.component_id().as_str(), reference.name().as_str(), &input.name),
            )?;
            Ok(Input {
                name: input.name.clone(),
                semantic_type: input.value_type.clone(),
                rust_type: parameter.source_type.to_string(),
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let selections = specgate_discovery::setup::resolve_setups(raw, &candidate.metadata().raw_registry)
        .map_err(|error| Error::wrap(super::ErrorKind::Linking, "candidate setup resolution failed", error))?;
    let mut setups = Vec::with_capacity(selections.len());
    for selection in &selections {
        let producer = selection.producer;
        if producer.is_method
            || producer.is_async
            || ((!producer.is_public || producer.test_target.is_some()) && producer.replay_output.is_none())
        {
            return failure(format!(
                "candidate setup '{}::{}' must be a public synchronous free function",
                producer.module_path, producer.fn_name
            ));
        }
        validate_ident(&producer.fn_name, "candidate setup function name")?;
        setups.push(super::model::Setup {
            module_path: if producer.test_target.is_some() {
                producer.module_path.as_str().split("::").skip(1).map(str::to_string).collect()
            } else {
                module_path(producer, &candidate.metadata().crate_name)?
            },
            fn_name: producer.fn_name.to_string(),
            fills: selection.fills.to_string(),
            native_module: producer.module_path.to_string(),
            replay_output: producer.replay_output.clone(),
            test_target: producer.test_target.clone(),
            inputs: producer
                .params
                .iter()
                .map(|parameter| {
                    inputs
                        .iter()
                        .position(|input| input.name == parameter.name.as_str())
                        .ok_or_else(|| Error::from(format!("setup input '{}' is absent from folded operation inputs", parameter.name)))
                })
                .collect::<Result<_, _>>()?,
        });
    }
    let parameters = raw
        .params
        .iter()
        .map(|parameter| {
            if let Some(index) = setups.iter().position(|setup| setup.fills == parameter.name.as_str()) {
                let native = parameter.ty.as_str().trim();
                let borrow = if native.starts_with("& mut ") || native.starts_with("&mut ") {
                    "&mut "
                } else if native.starts_with('&') {
                    "&"
                } else {
                    ""
                };
                Ok(super::model::Parameter::Setup {
                    index,
                    borrow: borrow.to_string(),
                })
            } else {
                inputs
                    .iter()
                    .position(|input| input.name == parameter.name.as_str())
                    .map(super::model::Parameter::Input)
                    .ok_or_else(|| {
                        Error::from(format!(
                            "native parameter '{}' is absent from folded operation inputs",
                            parameter.name
                        ))
                    })
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(Link {
        component_id: reference.component_id().as_str().into(),
        operation_name: reference.name().as_str().into(),
        module_path: operation_module,
        fn_name: raw.fn_name.to_string(),
        type_arguments: raw
            .type_arguments
            .iter()
            .map(|argument| {
                let ty = syn::parse_str::<syn::Type>(argument.as_str())
                    .map_err(|error| Error::from(format!("invalid candidate generic type argument '{argument}': {error}")))?;
                Ok(quote::quote!(#ty).to_string())
            })
            .collect::<Result<_, Error>>()?,
        inputs,
        output: reference.output().cloned(),
        is_async: raw.is_async,
        is_method: raw.is_method,
        setups,
        parameters,
    })
}

pub(super) fn validate_inputs(reference: &specgate_ctsc::replay::model::RegistryOp, candidate: &Operation) -> Result<(), Error> {
    let expected_names = reference.inputs().iter().map(|input| input.name.as_str()).collect::<Vec<_>>();
    let actual_names = candidate.inputs.iter().map(|input| input.name.as_str()).collect::<Vec<_>>();
    if expected_names != actual_names {
        let expected_set = expected_names.iter().copied().collect::<BTreeSet<_>>();
        let actual_set = actual_names.iter().copied().collect::<BTreeSet<_>>();
        let problem = if expected_set == actual_set {
            "input order differs"
        } else {
            "input names differ"
        };
        return failure(format!(
            "candidate operation '{}::{}' {problem}: expected {expected_names:?}, found {actual_names:?}",
            reference.component_id(),
            reference.name()
        ));
    }
    for (reference_input, candidate_input) in reference.inputs().iter().zip(&candidate.inputs) {
        let expected_type = semantic_name(
            &reference_input.value_type,
            ValueContext::new(reference.component_id().as_str(), reference.name().as_str(), &reference_input.name),
        )?;
        if candidate_input.ty != expected_type {
            return failure(format!(
                "candidate operation '{}::{}' input '{}' type mismatch: expected '{}', found '{}'",
                reference.component_id(),
                reference.name(),
                reference_input.name,
                expected_type,
                candidate_input.ty
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_output(reference: &specgate_ctsc::replay::model::RegistryOp, candidate: &Operation) -> Result<(), Error> {
    let expected = reference
        .output()
        .map(|value_type| {
            outcome_type_name(
                value_type,
                ValueContext::new(reference.component_id().as_str(), reference.name().as_str(), "$result"),
            )
        })
        .transpose()?;
    if candidate.output.as_ref().map(specgate_discovery::identity::TypeExpression::as_str) != expected.as_deref() {
        let detail = if reference.output().is_some_and(|ty| ty.primitive_name().is_none()) {
            "uses unsupported structured replay type mapping"
        } else {
            "output type mismatch"
        };
        return failure(format!(
            "candidate operation '{}::{}' {detail}: expected '{}', found '{}'",
            reference.component_id(),
            reference.name(),
            expected.as_deref().unwrap_or("unit"),
            candidate
                .output
                .as_ref()
                .map_or("unit", specgate_discovery::identity::TypeExpression::as_str)
        ));
    }
    if candidate.empty != reference.empty() {
        return failure(format!(
            "candidate operation '{}::{}' empty outcome mismatch: expected {}, found {}",
            reference.component_id(),
            reference.name(),
            reference.empty(),
            candidate.empty
        ));
    }
    let expected_errors = reference
        .errors()
        .iter()
        .map(|error| {
            let ty = error.value_type.as_ref().map_or_else(
                || Ok(String::new()),
                |value_type| {
                    outcome_type_name(
                        value_type,
                        ValueContext::new(reference.component_id().as_str(), reference.name().as_str(), "$error"),
                    )
                },
            )?;
            Ok((error.name.clone(), ty))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let candidate_errors = candidate
        .errors
        .iter()
        .map(|error| {
            (
                error.name.to_string(),
                error.ty.as_ref().map_or_else(String::new, ToString::to_string),
            )
        })
        .collect::<Vec<_>>();
    if candidate_errors != expected_errors {
        return failure(format!(
            "candidate operation '{}::{}' declared errors mismatch: expected {expected_errors:?}, found {candidate_errors:?}",
            reference.component_id(),
            reference.name()
        ));
    }
    Ok(())
}

fn validate_structured_types(bundle: &ReplayBundle, candidate: &Candidate, links: &[Link]) -> Result<(), Error> {
    let mut pending = Vec::new();
    for operation in &bundle.registry.operations {
        if links.iter().any(|link| {
            link.component_id.as_str() == operation.component_id().as_str() && link.operation_name.as_str() == operation.name().as_str()
        }) {
            for ty in operation
                .output()
                .into_iter()
                .chain(operation.errors().iter().filter_map(|error| error.value_type.as_ref()))
            {
                let value = serde_json::to_value(ty).map_err(|error| format!("failed to inspect replay outcome type: {error}"))?;
                named_references(&value, operation.component_id().as_str(), &mut pending)?;
            }
        }
    }
    if pending.is_empty() {
        return Ok(());
    }
    let reference: serde_json::Value = serde_json::from_slice(&bundle.registry_document)
        .map_err(|error| format!("failed to inspect linked reference registry: {error}"))?;
    let schema = serde_json::to_string(&candidate.schema).map_err(|error| format!("failed to encode candidate type schema: {error}"))?;
    let encoded = specgate_ctsc::registry::encode(
        "urn:ctsc:registry:replay-types",
        "0.1.0",
        specgate_ctsc::registry::Schema::new(&schema),
    )
    .map_err(|error| format!("failed to normalize candidate outcome declarations: {error}"))?;
    let actual: serde_json::Value = serde_json::from_str(&encoded.registry_json)
        .map_err(|error| format!("failed to inspect candidate outcome declarations: {error}"))?;
    let mut visited = BTreeSet::new();
    while let Some((component, name)) = pending.pop() {
        if !visited.insert((component.clone(), name.clone())) {
            continue;
        }
        let expected = type_declaration(&reference, &component, &name)
            .ok_or_else(|| format!("reference outcome type '{component}::{name}' has no linked declaration"))?;
        let found = type_declaration(&actual, &component, &name);
        if found != Some(expected) {
            return failure(format!(
                "candidate uses unsupported structured replay type mapping for '{component}::{name}': native declarations differ; explicit type mappings are not implemented"
            ));
        }
        named_references(expected, &component, &mut pending)?;
    }
    Ok(())
}

fn type_declaration<'a>(registry: &'a serde_json::Value, component: &str, name: &str) -> Option<&'a serde_json::Value> {
    registry["components"].as_array()?.iter().find(|entry| entry["id"] == component)?["types"]
        .as_array()?
        .iter()
        .find(|entry| entry["name"] == name)
}

fn named_references(value: &serde_json::Value, component: &str, pending: &mut Vec<(String, String)>) -> Result<(), Error> {
    match value {
        serde_json::Value::Object(fields) => {
            if value["kind"] == "named" {
                if value.get("registryId").is_some() {
                    return failure("candidate uses unsupported structured replay type mapping from an imported registry");
                }
                let name = value["name"]
                    .as_str()
                    .ok_or_else(|| "named replay outcome type has no name".to_string())?;
                let owner = value["componentId"].as_str().unwrap_or(component);
                pending.push((owner.to_string(), name.to_string()));
            }
            for child in fields.values() {
                named_references(child, component, pending)?;
            }
        }
        serde_json::Value::Array(values) => {
            for child in values {
                named_references(child, component, pending)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn outcome_type_name(value_type: &ReplayType, context: ValueContext<'_>) -> Result<String, Error> {
    if value_type.primitive_name().is_some() {
        return semantic_name(value_type, context).map(str::to_string);
    }
    if let Some((name, owner)) = value_type.named_parts() {
        return Ok(match owner.component_id() {
            Some(owner) if owner != context.component => format!("{owner}::{name}"),
            _ => name.to_string(),
        });
    }
    if let Some(item) = value_type.element_type() {
        let constructor = if value_type.kind_name() == "set" { "set" } else { "List" };
        return Ok(format!("{constructor}<{}>", outcome_type_name(item, context)?));
    }
    if let Some(value) = value_type.optional_value() {
        return Ok(format!("Option<{}>", outcome_type_name(value, context)?));
    }
    if let Some((keys, values)) = value_type.map_types() {
        return Ok(format!(
            "map<{}, {}>",
            outcome_type_name(keys, context)?,
            outcome_type_name(values, context)?
        ));
    }
    failure("inline record and tuple outcome type mappings are not supported by candidate discovery")
}

#[derive(Clone, Copy)]
pub(super) struct ValueContext<'a> {
    component: &'a str,
    operation: &'a str,
    name: &'a str,
}

impl<'a> ValueContext<'a> {
    fn new(component: &'a str, operation: &'a str, name: &'a str) -> Self {
        Self {
            component,
            operation,
            name,
        }
    }
}

pub(super) fn semantic_name<'a>(value_type: &'a ReplayType, context: ValueContext<'_>) -> Result<&'a str, Error> {
    let ValueContext {
        component,
        operation,
        name: value_name,
    } = context;
    let Some(name) = value_type.primitive_name() else {
        return failure(format!(
            "operation '{component}::{operation}' value '{value_name}' uses unsupported structured type"
        ));
    };
    match name {
        "unit" | "string" | "bool" | "i32" | "i64" | "u32" | "u64" | "f32" | "f64" => Ok(name),
        other => failure(format!(
            "operation '{component}::{operation}' value '{value_name}' uses unsupported primitive '{other}'"
        )),
    }
}

pub(super) fn check_rust(semantic_type: &ReplayType, rust_type: impl AsRef<str>, context: ValueContext<'_>) -> Result<(), Error> {
    let rust_type = rust_type.as_ref();
    let ValueContext {
        component,
        operation,
        name: input,
    } = context;
    let semantic = semantic_name(semantic_type, context)?;
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    let supported = match semantic {
        "unit" => native == "()",
        "string" => native == "String" || native == "&str",
        "bool" => native == "bool",
        "i32" => native == "i32",
        "i64" => native == "i64",
        "u32" => native == "u32",
        "u64" => native == "u64",
        "f32" => native == "f32",
        "f64" => native == "f64",
        _ => false,
    };
    if supported {
        Ok(())
    } else {
        failure(format!(
            "candidate operation '{component}::{operation}' input '{input}' uses unsupported Rust type '{rust_type}' for semantic type '{semantic}'"
        ))
    }
}

pub(super) fn module_path(raw: &RawOperation, package_name: impl AsRef<str>) -> Result<Vec<String>, Error> {
    let crate_ident = package_name.as_ref().replace('-', "_");
    let mut segments = raw.module_path.split("::").collect::<Vec<_>>();
    if segments.first().copied() != Some(crate_ident.as_str()) {
        return failure(format!(
            "candidate operation '{}::{}' raw module path '{}' does not begin with crate '{}'",
            raw.component, raw.name, raw.module_path, crate_ident
        ));
    }
    segments.remove(0);
    let mut result = Vec::with_capacity(segments.len());
    for segment in segments {
        validate_ident(segment, "candidate module path segment")?;
        result.push(segment.to_string());
    }
    Ok(result)
}

pub(super) fn validate_ident(value: impl AsRef<str>, label: impl AsRef<str>) -> Result<(), Error> {
    let value = value.as_ref();
    let label = label.as_ref();
    syn::parse_str::<syn::Ident>(value)
        .map(|_| ())
        .map_err(|error| format!("{label} '{value}' is not a supported Rust identifier: {error}").into())
}
