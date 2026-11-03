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
use super::storage::{ValueContext, check_rust};
use super::{
    BTreeSet, Candidate, Candidates, CommandEnvironment, Discovery, DiscoveryContext, DiscoveryInput, Input, Link, Operation, Path, Plan,
    PlannedOp, RawOperation, ReplayBundle, Scenario, Target,
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
    let value_bindings = super::binding::build(bundle, candidate, &links)?;
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
    if test_target.is_none() {
        for link in &links {
            if let Some(target) = candidate
                .metadata()
                .raw_registry
                .ops
                .iter()
                .find(|raw| raw.component == link.component_id && raw.name == link.operation_name && !raw.is_setup)
                .and_then(|raw| raw.test_target.as_ref())
            {
                if test_target.as_ref().is_some_and(|prior| prior != target) {
                    return failure("selected operations belong to different Cargo test targets");
                }
                test_target = Some(target.clone());
            }
        }
    }
    let plan = Plan {
        dispatch_modules: candidate
            .metadata()
            .raw_registry
            .ops
            .iter()
            .map(|operation| operation.module_path.to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        test_target,
        component_id: bundle.component_id.clone().as_str().into(),
        registry_id: bundle.registry.identity.id.as_str().to_string().into(),
        registry_version: bundle.registry.identity.version.as_str().to_string().into(),
        registry_digest: RegistryDigest::try_from(bundle.registry.identity.digest.as_str().to_string())?,
        registry_document: bundle.registry_document.clone(),
        value_bindings,
        target: Target {
            crate_name: candidate.metadata().crate_name.clone(),
            language: candidate.metadata().language.into(),
            package_name: candidate.metadata().package_name.clone(),
            package_version: candidate.metadata().package_version.clone(),
            package_root: candidate.metadata().package_root.clone(),
            runtime: candidate.metadata().runtime.clone(),
            executor: select_executor(candidate.metadata(), links.iter().any(|link| link.is_async))?,
        },
        links,
        scenarios,
    };
    serde_json::to_vec(&plan).map_err(|error| format!("failed to serialize target-local invocation plan: {error}"))?;
    Ok(plan)
}

fn select_executor(candidate: &super::CandidatesInner, asynchronous: bool) -> Result<super::model::Executor, Error> {
    use specgate_discovery::binding::{LanguageOptions, Runtime};
    let LanguageOptions::Rust { runtime, executor } = &candidate.executor_options else {
        return failure("candidate replay executor requires a Rust implementation");
    };
    if let Some(path) = executor {
        let parsed: syn::Path = syn::parse_str(path).map_err(|error| format!("invalid rust.executor path '{path}': {error}"))?;
        if parsed
            .segments
            .iter()
            .any(|segment| !matches!(segment.arguments, syn::PathArguments::None))
        {
            return failure("rust.executor must name a function without generic arguments");
        }
        return Ok(super::model::Executor::Custom { path: path.clone() });
    }
    if !asynchronous && *runtime == Runtime::Auto {
        return Ok(super::model::Executor::Synchronous);
    }
    let matches = candidate
        .executors
        .iter()
        .filter(|(_, source)| match runtime {
            Runtime::Auto => true,
            Runtime::Arty => source.package.as_str() == "arty",
            Runtime::Smol => source.package.as_str() == "smol",
            Runtime::Tokio => source.package.as_str() == "tokio",
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [(alias, dependency)] => Ok(super::model::Executor::Builtin {
            alias: (*alias).clone(),
            features: dependency.features.clone(),
            source: dependency
                .source
                .as_ref()
                .map_err(|diagnostic| {
                    Error::from(format!(
                        "candidate executor cannot be used by a generated runner: {diagnostic}; configure rust.executor"
                    ))
                })?
                .clone(),
        }),
        [] => failure(
            "no matching replay executor in the candidate's direct dependencies; add arty, smol, or tokio, or configure rust.executor",
        ),
        _ => failure("candidate has multiple matching replay executors; select rust.runtime or configure rust.executor"),
    }
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
    if raw.consumes_receiver {
        return failure(format!(
            "candidate operation '{}::{}' consumes its receiver; replay requires a borrowed &self or &mut self receiver",
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
            explicit_component: producer.explicit_component,
            module_path: module_path(producer, &candidate.metadata().crate_name)?,
            fn_name: producer.fn_name.to_string(),
            fills: selection.fills.to_string(),
            declared_fills: producer.fills.to_string(),
            native_module: producer.module_path.to_string(),
            replay_output: producer.replay_output.clone(),
            test_target: producer.test_target.clone(),
            inputs: specgate_discovery::setup::raw_parameters(producer, &candidate.metadata().raw_registry)
                .map_err(|error| Error::wrap(super::ErrorKind::Linking, "candidate setup input resolution failed", error))?
                .iter()
                .map(|parameter| {
                    inputs
                        .iter()
                        .position(|input| input.name == parameter.name.as_str())
                        .ok_or_else(|| Error::from(format!("setup input '{}' is absent from folded operation inputs", parameter.name)))
                })
                .collect::<Result<_, _>>()?,
            parameters: producer
                .params
                .iter()
                .map(|parameter| native_parameter(producer, parameter, &candidate.metadata().raw_registry, &inputs))
                .collect::<Result<_, _>>()?,
        });
    }
    let parameters = raw
        .params
        .iter()
        .map(|parameter| {
            if let Some(index) = setups.iter().position(|setup| setup.fills == parameter.name.as_str()) {
                let native = syn::parse_str::<syn::Type>(parameter.ty.as_str())
                    .map_err(|error| Error::from(format!("invalid candidate parameter type '{}': {error}", parameter.ty)))?;
                let projection = match &native {
                    syn::Type::Reference(reference)
                        if matches!(reference.elem.as_ref(), syn::Type::Slice(_))
                            || matches!(reference.elem.as_ref(), syn::Type::Path(path) if path.path.is_ident("str")) =>
                    {
                        let target = &reference.elem;
                        Some(quote::quote!(#target).to_string())
                    }
                    _ => None,
                };
                let borrow = match native {
                    syn::Type::Reference(reference) if reference.mutability.is_some() => "&mut ",
                    syn::Type::Reference(_) => "&",
                    _ => "",
                };
                Ok(super::model::Parameter::Setup {
                    index,
                    borrow: borrow.to_string(),
                    projection,
                })
            } else {
                native_parameter(raw, parameter, &candidate.metadata().raw_registry, &inputs)
            }
        })
        .collect::<Result<_, _>>()?;
    Ok(Link {
        explicit_component: raw.explicit_component,
        native_module: raw.module_path.to_string(),
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

fn native_parameter(
    operation: &specgate_discovery::registry::Operation,
    parameter: &specgate_discovery::registry::Field,
    registry: &specgate_discovery::registry::Registry,
    inputs: &[Input],
) -> Result<super::model::Parameter, Error> {
    if let Some(properties) = specgate_discovery::setup::property_setup(operation, parameter.ty.as_str(), registry)
        .map_err(|error| Error::wrap(super::ErrorKind::Linking, "candidate property setup resolution failed", error))?
    {
        let native: syn::Type =
            syn::parse_str(parameter.ty.as_str()).map_err(|error| Error::from(format!("invalid property setup type: {error}")))?;
        let borrow = match native {
            syn::Type::Reference(reference) if reference.mutability.is_some() => "&mut ",
            syn::Type::Reference(_) => "&",
            _ => "",
        };
        let mut scoped = operation.clone();
        scoped.module_path = properties.module_path.clone().into();
        let fields = properties
            .fields
            .iter()
            .map(|field| {
                let value = match native_parameter(&scoped, field, registry, inputs)? {
                    super::model::Parameter::Input(index) => super::model::PropertyValue::Input(index),
                    super::model::Parameter::Properties { fields, .. } => super::model::PropertyValue::Properties(fields),
                    super::model::Parameter::Setup { .. } => unreachable!("property fields do not select explicit producers"),
                };
                Ok((field.name.to_string(), value))
            })
            .collect::<Result<_, Error>>()?;
        Ok(super::model::Parameter::Properties {
            rust_type: format!("{}::{}", properties.module_path, properties.native_name),
            borrow: borrow.to_string(),
            fields,
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
    Ok(())
}

pub(super) fn validate_output(reference: &specgate_ctsc::replay::model::RegistryOp, candidate: &Operation) -> Result<(), Error> {
    if candidate.output.is_some() != reference.output().is_some() {
        return failure(format!(
            "candidate operation '{}::{}' output type mismatch: result channels differ",
            reference.component_id(),
            reference.name(),
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
        .map(|error| (error.name.clone(), error.value_type.is_some()))
        .collect::<Vec<_>>();
    let candidate_errors = candidate
        .errors
        .iter()
        .map(|error| (error.name.to_string(), error.ty.is_some()))
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

pub(super) fn module_path(raw: &RawOperation, package_name: impl AsRef<str>) -> Result<Vec<String>, Error> {
    let mut segments = raw.module_path.split("::").collect::<Vec<_>>();
    let library_root = package_name.as_ref().replace('-', "_");
    let crate_ident = raw
        .test_target
        .as_ref()
        .filter(|target| target.kind != specgate_discovery::registry::TestTargetKind::Library)
        .map(|target| target.name.replace('-', "_"))
        .filter(|root| segments.first().copied() == Some(root.as_str()))
        .unwrap_or(library_root);
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
