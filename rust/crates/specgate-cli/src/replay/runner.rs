//! Deterministic candidate-runner generation and execution.
//!
//! A runner receives a validated static plan, emits source with stable IDs and
//! timestamps, executes in an isolated scratch package, and returns native
//! captures. Filesystem and process effects flow through concrete injectable
//! boundaries; malformed plans are programming-invariant failures.
use super::*;

mod bridge;

// Generated source uses serde_json 1.x APIs only; retaining the major range
// lets Cargo select compatible fixes without changing the runner contract.
const SERDE_VERSION: &str = "1";
// Candidate runner IDs reserve the `2` namespace and fixed hexadecimal widths
// required by native capture. Each generated scenario starts 1 ms after the
// previous one, while the run starts 10 ms earlier, leaving deterministic room
// for ordinary fixture spans without coupling timestamps to wall-clock time.
// CTSC validators require only ordered, non-overlapping ancestry; changing
// these values changes generated traces and therefore golden compatibility.
const ID_PREFIX: &str = "2";
const TRACE_WIDTH: usize = 31;
const SPAN_WIDTH: usize = 13;
const RUN_SUFFIX: &str = "01";
const SCENARIO_SUFFIX: &str = "02";
const TIME_STRIDE: i64 = 1_000_000;
const FIRST_IDENTITY: u64 = 1;
const START_OFFSET: i64 = 10_000_000;
const CLOCK_STEP: i64 = 1;

pub(super) fn execute_with(plan: &Plan, execution: &Execution, system: &CommandEnvironment) -> Result<Vec<Capture>, Error> {
    let scratch = scratch(system)?;
    execute_in(plan, scratch.path(), execution, system)
}

pub(super) fn execute_in(
    plan: &Plan,
    scratch: impl AsRef<Path>,
    execution: &Execution,
    system: &CommandEnvironment,
) -> Result<Vec<Capture>, Error> {
    let scratch = scratch.as_ref();
    if plan.test_target.is_some() {
        return execute_harness(plan, scratch, execution, system);
    }
    system
        .create_dir_all(scratch.join("src"))
        .map_err(|error| format!("failed to scaffold replay runner {}: {error}", scratch.display()))?;
    let plan_path = scratch.join("plan.json");
    let plan_json = serde_json::to_vec(plan).map_err(|error| format!("failed to serialize target-local invocation plan: {error}"))?;
    system
        .write(&plan_path, plan_json)
        .map_err(|error| format!("failed to write replay invocation plan {}: {error}", plan_path.display()))?;

    let cargo = cargo_manifest(plan)?;
    system
        .write(scratch.join("Cargo.toml"), cargo.manifest)
        .map_err(|error| format!("failed to write replay runner manifest: {error}"))?;
    let registry_config = cargo
        .config
        .map(|config| {
            let path = scratch.join("registry-config.toml");
            system
                .write(&path, config)
                .map_err(|error| format!("failed to write replay registry config: {error}"))?;
            Ok::<_, Error>(path)
        })
        .transpose()?;
    let source = source(plan);
    system
        .write(scratch.join("src").join("main.rs"), source)
        .map_err(|error| format!("failed to write replay runner source: {error}"))?;

    let sidecar = scratch.join("candidate-captures.json");
    let _ = system.remove_file(&sidecar);
    let mut request = ProcessRequest::builder(cargo_bin(system)).arg("run").arg("--quiet");
    if let Some(config) = registry_config {
        request = request.arg("--config").arg(config);
    }
    let mut request = request
        .arg("--manifest-path")
        .arg(scratch.join("Cargo.toml"))
        .arg("--")
        .arg(&sidecar)
        .current_dir(&plan.target.package_root)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", scratch.join("target"));
    if plan.target.unhashed_library {
        request = request.env("CARGO_BUILD_BUILD_DIR", scratch.join("build"));
    }
    let request = request.build();
    let output = execution.run(&request).map_err(|error| {
        format!(
            "failed to run candidate replay runner: {error}\n{}",
            request.diagnostic(None, system)
        )
    })?;
    if !output.status.success() {
        return failure(format!(
            "candidate replay runner failed:\n{}",
            request.diagnostic(Some(&output), system)
        ));
    }
    let captures_json = system
        .read(&sidecar)
        .map_err(|error| format!("candidate replay runner did not write native capture sidecar: {error}"))?;
    let captures: Vec<Capture> =
        serde_json::from_slice(&captures_json).map_err(|error| format!("candidate replay native capture sidecar is malformed: {error}"))?;
    if captures.len() != plan.scenarios.len() {
        return failure(format!(
            "candidate replay runner produced {} captures for {} planned scenarios",
            captures.len(),
            plan.scenarios.len()
        ));
    }
    Ok(captures)
}

pub(super) fn scratch(system: &CommandEnvironment) -> Result<crate::system::Scratch, Error> {
    system
        .scratch(None::<&Path>, "specgate-replay-candidate-")
        .map_err(|error| Error::from(format!("failed to create replay scratch directory: {error}")))
}

pub(super) fn cargo_manifest(plan: &Plan) -> Result<specgate_discovery::runner::RunnerCargo, Error> {
    let serde_json = specgate_discovery::runner::Dependency::builder("serde_json")
        .version(SERDE_VERSION)
        .build()
        .expect("the fixed serde_json replay dependency must be valid");
    let mut dependencies = BTreeMap::from([
        (
            "candidate".to_string(),
            specgate_discovery::runner::Dependency::local(
                plan.target.package_name.clone(),
                plan.target.package_version.clone(),
                plan.target.package_root.clone(),
            )
            .map_err(|error| Error::wrap(ErrorKind::Linking, "candidate replay dependency is invalid", error))?,
        ),
        ("serde_json".to_string(), serde_json),
        (
            "specgate_runtime".to_string(),
            specgate_discovery::runner::Dependency::from_source(&plan.target.runtime)
                .map_err(|error| Error::wrap(ErrorKind::Linking, "runtime replay dependency is invalid", error))?,
        ),
    ]);
    if let model::Executor::Builtin { alias, source, features } = &plan.target.executor {
        if dependencies.contains_key(alias) {
            return failure(format!(
                "candidate executor dependency alias '{alias}' conflicts with a generated replay dependency"
            ));
        }
        let mut dependency = specgate_discovery::runner::Dependency::from_source(source)
            .map_err(|error| Error::wrap(ErrorKind::Linking, "executor replay dependency is invalid", error))?;
        dependency.features.clone_from(features);
        dependency.default_features = false;
        dependencies.insert(alias.clone(), dependency);
    }
    let cargo = specgate_discovery::runner::runner_cargo("specgate-replay-runner", dependencies)
        .map_err(|error| Error::wrap(ErrorKind::Linking, "replay runner manifest is invalid", error))?;
    Ok(cargo)
}

pub(super) fn source(plan: &Plan) -> String {
    source_in(plan, false)
}

fn source_in(plan: &Plan, harness: bool) -> String {
    let asynchronous = harness || !matches!(plan.target.executor, model::Executor::Synchronous);
    let mut source = if harness {
        String::from("{\nfn run(sidecar: &std::path::Path) -> Result<(), String> {\n")
    } else {
        String::from(
            "fn run(sidecar: &std::path::Path, publish: impl FnOnce(&std::path::Path, &[u8]) -> std::io::Result<()> + Send) -> Result<(), String> {\n",
        )
    };
    source.push_str("    extern crate alloc;\n");
    if asynchronous {
        match &plan.target.executor {
            model::Executor::Synchronous => source.push_str("    __specgate::harness::run_sync(async {\n"),
            model::Executor::Custom { path } => {
                let path = if harness {
                    path.clone()
                } else {
                    path.replacen("crate::", "candidate::", 1)
                };
                writeln!(source, "    {path}(move || async move {{").expect("writing source");
            }
            model::Executor::Builtin {
                alias, source: dependency, ..
            } => match dependency.package.as_str() {
                "arty" => {
                    writeln!(source, "    let runtime = {alias}::runtime::Runtime::new().map_err(|error| format!(\"failed to start Arty replay executor: {{error}}\"))?;\n    let result = runtime.scheduler().block_on(async move |_| {{").expect("writing source");
                }
                "smol" => writeln!(source, "    {alias}::block_on(async {{").expect("writing source"),
                "tokio" => {
                    writeln!(source, "    let runtime = {alias}::runtime::Builder::new_current_thread().enable_all().build().map_err(|error| format!(\"failed to start Tokio replay executor: {{error}}\"))?;\n    runtime.block_on(async {{").expect("writing source");
                }
                _ => unreachable!("validated replay executor"),
            },
        }
    }
    source.push_str("    let mut captures = Vec::new();\n");
    // Zero identifies no scenario in generated native-capture IDs; the first
    // scenario therefore starts at one, which feeds every deterministic trace
    // and span identifier below.
    for (scenario_position, scenario) in plan.scenarios.iter().enumerate() {
        source.push_str("    {\n");
        let identity = u64::try_from(scenario_position)
            .expect("validated replay scenario position must fit u64")
            .checked_add(FIRST_IDENTITY)
            .expect("validated replay scenario identity must not overflow");
        let trace_id = format!("{ID_PREFIX}{identity:0TRACE_WIDTH$x}");
        let span_body = format!("{ID_PREFIX}{identity:0SPAN_WIDTH$x}");
        let run_span_id = format!("{span_body}{RUN_SUFFIX}");
        let scenario_span_id = format!("{span_body}{SCENARIO_SUFFIX}");
        let start_time = i64::try_from(scenario_position)
            .expect("validated replay scenario position must fit i64")
            .checked_mul(TIME_STRIDE)
            .and_then(|value| value.checked_add(START_OFFSET))
            .expect("validated replay scenario timestamp must not overflow");
        source.push_str(
            "    specgate_runtime::capture::start(\n        specgate_runtime::capture::Config::builder(specgate_runtime::capture::ConfigDeps {\n",
        );
        write!(
            source,
            "            scenario_name: {}.to_string(),\n            trace_id: specgate_runtime::capture::TraceId::try_from({}).map_err(|error| error.to_string())?,\n            run_id: specgate_runtime::capture::SpanId::try_from({}).map_err(|error| error.to_string())?,\n            scenario_id: specgate_runtime::capture::SpanId::try_from({}).map_err(|error| error.to_string())?,\n        }})\n        .start_time({start_time})\n        .clock_step({CLOCK_STEP})\n        .build().map_err(|error| error.to_string())?,\n    ).map_err(|error| error.to_string())?;\n",
            string_literal(&scenario.name),
            string_literal(&trace_id),
            string_literal(&run_span_id),
            string_literal(&scenario_span_id),
        )
        .expect("writing to a String cannot fail");
        let mut receivers: Vec<(model::Setup, Vec<ReplayInput>, String)> = Vec::new();
        for (operation_index, operation) in scenario.operations.iter().enumerate() {
            let link = plan
                .links
                .get(operation.link_index)
                .expect("replay plan operation link index must reference a validated link");
            assert_eq!(
                operation.inputs.len(),
                link.inputs.len(),
                "replay planned operation '{}::{}' must have one value per validated input",
                link.component_id,
                link.operation_name
            );
            let rendered_inputs = operation
                .inputs
                .iter()
                .zip(&link.inputs)
                .map(|(input, definition)| {
                    assert!(
                        input.name == definition.name && input.value_type == definition.semantic_type,
                        "replay planned input '{}: {:?}' must match validated link input '{}: {:?}'",
                        input.name,
                        input.value_type,
                        definition.name,
                        definition.semantic_type
                    );
                    render_value(&input.value, &definition.rust_type)
                })
                .collect::<Vec<_>>();
            let mut setup_locals = Vec::with_capacity(link.setups.len());
            for (setup_index, setup) in link.setups.iter().enumerate() {
                let values = setup
                    .inputs
                    .iter()
                    .map(|index| operation.inputs[*index].clone())
                    .collect::<Vec<_>>();
                let receiver = setup.fills.is_empty() && link.is_method;
                if receiver
                    && let Some((_, _, local)) = receivers.iter().find(|(prior, prior_values, _)| {
                        prior.module_path == setup.module_path && prior.fn_name == setup.fn_name && prior_values == &values
                    })
                {
                    setup_locals.push(local.clone());
                    continue;
                }
                let local = format!("__sg_setup_{scenario_position}_{operation_index}_{setup_index}");
                let arguments = setup
                    .inputs
                    .iter()
                    .map(|index| rendered_inputs[*index].as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if harness {
                    let values = setup
                        .inputs
                        .iter()
                        .map(|index| format!("__specgate::ToNativeValue::to_native_value(&({}))", rendered_inputs[*index]))
                        .collect::<Vec<_>>()
                        .join(", ");
                    writeln!(source, "    let mut {local} = __specgate::harness::setup({}, {}, {}, {}, {}, vec![{values}]).map_err(|error| error.to_string())?;",
                        string_literal(link.component_id.as_str()), string_literal(link.operation_name.as_str()),
                        string_literal(&setup.native_module), string_literal(&setup.fn_name), string_literal(&setup.declared_fills),
                    ).expect("writing to a String cannot fail");
                } else {
                    let path = invocation_path(&setup.module_path, &setup.fn_name);
                    writeln!(source, "    let mut {local} = {path}({arguments});").expect("writing to a String cannot fail");
                }
                if receiver {
                    receivers.push((setup.clone(), values, local.clone()));
                }
                setup_locals.push(local);
            }
            if harness {
                let inputs = rendered_inputs
                    .iter()
                    .map(|value| format!("__specgate::ToNativeValue::to_native_value(&({value}))"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let injected = link
                    .parameters
                    .iter()
                    .filter_map(|parameter| match parameter {
                        model::Parameter::Setup { index, .. } => Some(setup_locals[*index].as_str()),
                        model::Parameter::Input(_) | model::Parameter::Properties { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let (dispatch, receiver) = if link.is_method {
                    let index = link
                        .setups
                        .iter()
                        .position(|setup| setup.fills.is_empty())
                        .expect("validated receiver");
                    let setup = &link.setups[index];
                    (
                        format!(
                            "__specgate::harness::setup_invocation({}, {}, {}, {}, {})",
                            string_literal(link.component_id.as_str()),
                            string_literal(link.operation_name.as_str()),
                            string_literal(&setup.native_module),
                            string_literal(&setup.fn_name),
                            string_literal(&setup.declared_fills)
                        ),
                        format!("&mut *{}", setup_locals[index]),
                    )
                } else {
                    (
                        format!(
                            "__specgate::harness::operation_invocation({}, {})",
                            string_literal(link.component_id.as_str()),
                            string_literal(link.operation_name.as_str())
                        ),
                        "&mut ()".to_string(),
                    )
                };
                writeln!(source, "    {dispatch}.map_err(|error| error.to_string())?({receiver}, vec![{inputs}], vec![{injected}]).await.map_err(|error| error.to_string())?;").expect("writing source");
                continue;
            }
            let arguments = link
                .parameters
                .iter()
                .map(|parameter| match parameter {
                    model::Parameter::Properties { rust_type, borrow, fields } => {
                        let mut ty: syn::Type = syn::parse_str(rust_type).expect("validated property setup type");
                        qualify_type(&mut ty, &plan.target.crate_name);
                        let ty = quote::quote!(#ty).to_string();
                        let values = fields.iter().map(|(name, index)| format!(
                            "({}.to_string(), specgate_runtime::ToNativeValue::to_native_value(&({})))",
                            string_literal(name), rendered_inputs[*index]
                        )).collect::<Vec<_>>().join(", ");
                        format!("{borrow}<{ty} as specgate_runtime::harness::Input>::decode(specgate_runtime::Value::Map([{values}].into_iter().collect())).map_err(|error| error.to_string())?")
                    }
                    model::Parameter::Input(index) => rendered_inputs[*index].clone(),
                    model::Parameter::Setup { index, borrow, .. } => format!("{borrow}{}", setup_locals[*index]),
                })
                .collect::<Vec<_>>()
                .join(", ");
            let mut path = if link.is_method {
                let index = link
                    .setups
                    .iter()
                    .position(|setup| setup.fills.is_empty())
                    .expect("a validated method link must have a receiver producer");
                format!("{}.{}", setup_locals[index], link.fn_name)
            } else {
                invocation_path(&link.module_path, &link.fn_name)
            };
            if !link.type_arguments.is_empty() {
                let arguments = link
                    .type_arguments
                    .iter()
                    .map(|argument| {
                        let mut ty = syn::parse_str::<syn::Type>(argument).expect("validated native generic argument");
                        qualify_type(&mut ty, &plan.target.crate_name);
                        quote::quote!(#ty).to_string()
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(path, "::<{arguments}>").expect("writing to a String cannot fail");
            }
            let await_suffix = if link.is_async { ".await" } else { "" };
            writeln!(source, "    let _ = {path}({arguments}){await_suffix};").expect("writing to a String cannot fail");
        }
        source.push_str("    captures.push(specgate_runtime::capture::finish().map_err(|error| error.to_string())?);\n");
        source.push_str("    }\n");
    }
    if harness {
        source.push_str("    __specgate::harness::publish(sidecar, &captures).map_err(|error| error.to_string())?;\n    Ok(())\n");
    } else {
        source.push_str("    let json = serde_json::to_vec(&captures).map_err(|error| format!(\"failed to serialize native captures: {error}\"))?;\n    publish(sidecar, &json).map_err(|error| format!(\"failed to write native capture sidecar: {error}\"))?;\n    Ok(())\n");
    }
    if asynchronous {
        match &plan.target.executor {
            model::Executor::Synchronous => source.push_str("    }).map_err(|error| error.to_string())?\n"),
            model::Executor::Builtin { source: dependency, .. } if dependency.package.as_str() == "arty" => {
                source.push_str("    });\n    runtime.stop().map_err(|error| format!(\"failed to stop Arty replay executor: {error}\"))?;\n    result.map_err(|error| format!(\"Arty replay task failed: {error}\"))?\n");
            }
            _ => source.push_str("    })\n"),
        }
    }
    if harness {
        source.push_str("}\nlet sidecar = std::env::var_os(\"SPECGATE_TEST_CAPTURE\").expect(\"missing test replay output\");\nrun(std::path::Path::new(&sidecar)).expect(\"candidate test replay failed\");\n}\n");
        let target = plan.test_target.as_ref().expect("test-harness generation requires a target");
        let root = if target.kind == specgate_discovery::registry::TestTargetKind::Library {
            "crate".to_string()
        } else {
            plan.target.crate_name.clone()
        };
        let mut block: syn::Block = syn::parse_str(&source).expect("generated test replay source is a Rust block");
        let root: syn::Path = syn::parse_str(&root).expect("validated candidate crate path");
        syn::visit_mut::VisitMut::visit_block_mut(&mut HarnessPaths { root }, &mut block);
        source = quote::quote!(#block).to_string();
    } else {
        source.push_str("}\n\nfn main() -> Result<(), String> {\n    let sidecar = std::env::args_os().nth(1).ok_or_else(|| \"missing capture sidecar path\".to_string())?;\n    run(std::path::Path::new(&sidecar), |path, bytes| std::fs::write(path, bytes))\n}\n");
    }

    source
}

struct HarnessPaths {
    root: syn::Path,
}

impl syn::visit_mut::VisitMut for HarnessPaths {
    fn visit_path_mut(&mut self, i: &mut syn::Path) {
        syn::visit_mut::visit_path_mut(self, i);
        if i.segments.first().is_some_and(|segment| segment.ident == "candidate") {
            i.segments.first_mut().expect("nonempty candidate path").ident = self.root.segments[0].ident.clone();
        } else if i.segments.first().is_some_and(|segment| segment.ident == "specgate_runtime")
            && i.segments.get(1).is_some_and(|segment| segment.ident == "capture")
        {
            let mut path: syn::Path = syn::parse_quote!(__specgate);
            path.segments.extend(i.segments.iter().skip(2).cloned());
            *i = path;
        }
    }
}

fn execute_harness(plan: &Plan, scratch: &Path, execution: &Execution, system: &CommandEnvironment) -> Result<Vec<Capture>, Error> {
    system
        .create_dir_all(scratch)
        .map_err(|error| format!("failed to scaffold test replay: {error}"))?;
    let entry = scratch.join("test-entry.rs");
    system
        .write(&entry, source_in(plan, true))
        .map_err(|error| format!("failed to write test replay entry: {error}"))?;
    let dispatch = scratch.join("test-dispatch.rs");
    system
        .write(&dispatch, bridge::source(plan, None))
        .map_err(|error| format!("failed to write lexical replay dispatch: {error}"))?;
    let module_dispatches = plan
        .dispatch_modules
        .iter()
        .enumerate()
        .map(|(index, module)| {
            let path = scratch.join(format!("test-module-{index}.rs"));
            system
                .write(&path, bridge::source(plan, Some(module)))
                .map_err(|error| format!("failed to write module-local replay dispatch for '{module}': {error}"))?;
            Ok((module.clone(), path))
        })
        .collect::<Result<BTreeMap<_, _>, Error>>()?;
    let binaries = specgate_discovery::runner::harness::build(
        &plan.target.package_root,
        &entry,
        &dispatch,
        &module_dispatches,
        scratch,
        plan.test_target.as_ref(),
    )
    .map_err(|error| Error::wrap(ErrorKind::Linking, "failed to build candidate test replay", error))?;
    let target = plan.test_target.as_ref().expect("test replay requires a selected target");
    let binary = binaries
        .iter()
        .find(|binary| &binary.target == target)
        .ok_or_else(|| Error::from(format!("candidate test target '{}' has no generated replay entry", target.name)))?;
    let sidecar = scratch.join("candidate-captures.json");
    let request = ProcessRequest::builder(&binary.executable)
        .arg(&binary.entry)
        .args(["--exact", "--test-threads=1", "--nocapture"])
        .env("SPECGATE_TEST_CAPTURE", &sidecar)
        .env_remove("SPECGATE_NATIVE_CAPTURE")
        .current_dir(&plan.target.package_root)
        .build();
    let output = execution
        .run(&request)
        .map_err(|error| format!("failed to run candidate test replay: {error}\n{}", request.diagnostic(None, system)))?;
    if !output.status.success() {
        return failure(format!(
            "candidate test replay failed:\n{}",
            request.diagnostic(Some(&output), system)
        ));
    }
    let bytes = system
        .read(&sidecar)
        .map_err(|error| format!("candidate test replay wrote no captures: {error}"))?;
    let captures: Vec<Capture> =
        serde_json::from_slice(&bytes).map_err(|error| format!("candidate test replay wrote malformed captures: {error}"))?;
    if captures.len() != plan.scenarios.len() {
        return failure("candidate test replay produced a different number of scenarios than planned");
    }
    Ok(captures)
}

fn invocation_path(module_path: &[String], function: &str) -> String {
    std::iter::once("candidate")
        .chain(module_path.iter().map(String::as_str))
        .chain(std::iter::once(function))
        .collect::<Vec<_>>()
        .join("::")
}

fn qualify_type(ty: &mut syn::Type, native_crate: &str) {
    struct NativePaths<'a>(&'a str);
    impl syn::visit_mut::VisitMut for NativePaths<'_> {
        fn visit_path_mut(&mut self, i: &mut syn::Path) {
            syn::visit_mut::visit_path_mut(self, i);
            if let Some(first) = i.segments.first_mut()
                && (first.ident == self.0 || first.ident == "crate")
            {
                first.ident = syn::parse_quote!(candidate);
            }
        }
    }
    syn::visit_mut::VisitMut::visit_type_mut(&mut NativePaths(native_crate), ty);
}

pub(super) fn render_value(value: &ReplayValue, rust_type: impl AsRef<str>) -> String {
    struct Collections;
    impl syn::visit_mut::VisitMut for Collections {
        fn visit_type_mut(&mut self, i: &mut syn::Type) {
            if let syn::Type::Path(path) = i
                && path.qself.is_none()
                && path.path.segments.len() == 1
                && matches!(
                    path.path.segments[0].ident.to_string().as_str(),
                    "BTreeMap" | "HashMap" | "BTreeSet" | "HashSet"
                )
            {
                let segment = &path.path.segments[0];
                path.path = syn::parse_quote!(::std::collections::#segment);
            }
            syn::visit_mut::visit_type_mut(self, i);
        }
    }
    let rust_type = rust_type.as_ref();
    let mut qualified: syn::Type = syn::parse_str(rust_type).expect("validated replay input type");
    syn::visit_mut::VisitMut::visit_type_mut(&mut Collections, &mut qualified);
    let qualified = quote::quote!(#qualified).to_string();
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
    match value {
        ReplayValue::List(values) => return render_collection(values, rust_type, &qualified),
        ReplayValue::Set(values) => return render_collection(values, rust_type, &qualified),
        ReplayValue::Map(values) => {
            let (_, arguments) = link::native_arguments(rust_type).expect("validated map input");
            let elements = values
                .iter()
                .map(|(key, value)| format!("(String::from({}), {})", string_literal(key), render_value(value, &arguments[1])))
                .collect::<Vec<_>>()
                .join(", ");
            return format!("[{elements}].into_iter().collect::<{qualified}>()");
        }
        ReplayValue::Optional(value) => {
            let (_, arguments) = link::native_arguments(rust_type).expect("validated optional input");
            return value.as_ref().map_or_else(
                || format!("<{qualified}>::None"),
                |value| format!("Some({})", render_value(value, &arguments[0])),
            );
        }
        _ => {}
    }
    match (value, native.as_str()) {
        (ReplayValue::Unit, "()") => "()".to_string(),
        (ReplayValue::String(value), "String") => format!("String::from({})", string_literal(value)),
        (ReplayValue::String(value), "&str") => string_literal(value),
        (ReplayValue::Bool(value), "bool") => value.to_string(),
        (ReplayValue::I32(value), "i32") => format!("{value}_i32"),
        (ReplayValue::I64(value), "i64") => format!("{value}_i64"),
        (ReplayValue::U32(value), "u32") => format!("{value}_u32"),
        (ReplayValue::U64(value), "u64") => format!("{value}_u64"),
        (ReplayValue::F32Bits(bits), "f32") => format!("f32::from_bits({bits}_u32)"),
        (ReplayValue::F64Bits(bits), "f64") => format!("f64::from_bits({bits}_u64)"),
        _ => panic!("validated replay value {value:?} cannot be passed losslessly to linked Rust type '{rust_type}'"),
    }
}

fn render_collection<'a>(values: impl IntoIterator<Item = &'a ReplayValue>, rust_type: &str, qualified: &str) -> String {
    let (_, arguments) = link::native_arguments(rust_type).expect("validated collection input");
    let elements = values
        .into_iter()
        .map(|value| render_value(value, &arguments[0]))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{elements}].into_iter().collect::<{qualified}>()")
}

pub(super) fn string_literal(value: impl AsRef<str>) -> String {
    let value = value.as_ref();
    let mut literal = String::with_capacity(value.len() + 2);
    literal.push('"');
    for character in value.chars() {
        match character {
            '\0' => literal.push_str("\\0"),
            '\u{8}' => literal.push_str("\\x08"),
            '\u{c}' => literal.push_str("\\x0c"),
            '"' => literal.push_str("\\\""),
            '\\' => literal.push_str("\\\\"),
            '\r' => literal.push_str("\\r"),
            '\n' => literal.push_str("\\n"),
            '\t' => literal.push_str("\\t"),
            character if character.is_control() => {
                write!(literal, "\\u{{{:x}}}", u32::from(character)).expect("writing to a String cannot fail");
            }
            character => literal.push(character),
        }
    }
    literal.push('"');
    literal
}

pub(super) fn cargo_bin(system: &CommandEnvironment) -> std::ffi::OsString {
    system.environment("CARGO").unwrap_or_else(|| "cargo".into())
}

pub(super) fn write_atomic(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>, system: &CommandEnvironment) -> Result<(), Error> {
    let path = path.as_ref();
    let bytes = bytes.as_ref();
    let filename = path.file_name().and_then(|name| name.to_str()).unwrap_or("candidate.otlp.json");
    system
        .publish(path, bytes, format!(".{filename}.specgate-replay-"))
        .map_err(|error| Error::from(format!("failed to publish replay output {}: {error}", path.display())))
}
