//! Deterministic candidate-runner generation and execution.
//!
//! A runner receives a validated static plan, emits source with stable IDs and
//! timestamps, executes in an isolated scratch package, and returns native
//! captures. Filesystem and process effects flow through concrete injectable
//! boundaries; malformed plans are programming-invariant failures.
use super::*;

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
    let request = request
        .arg("--manifest-path")
        .arg(scratch.join("Cargo.toml"))
        .arg("--")
        .arg(&sidecar)
        .current_dir(&plan.target.package_root)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .build();
    let output = execution
        .run(&request)
        .map_err(|error| format!("failed to run candidate replay runner: {error}"))?;
    if !output.status.success() {
        return failure(format!(
            "candidate replay runner failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
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
    let dependencies = BTreeMap::from([
        (
            "candidate".to_string(),
            specgate_discovery::runner::Dependency::local(
                plan.target.package_name.clone(),
                plan.target.package_version.clone(),
                plan.target.package_root.clone(),
            )
            .map_err(|error| error.to_string())?,
        ),
        ("serde_json".to_string(), serde_json),
        (
            "specgate_runtime".to_string(),
            specgate_discovery::runner::Dependency::from_source(&plan.target.runtime).map_err(|error| error.to_string())?,
        ),
    ]);
    specgate_discovery::runner::runner_cargo("specgate-replay-runner", dependencies).map_err(|error| Error::from(error.to_string()))
}

pub(super) fn source(plan: &Plan) -> String {
    let mut source = String::from(
        "fn run(sidecar: &std::path::Path, publish: impl FnOnce(&std::path::Path, &[u8]) -> std::io::Result<()>) -> Result<(), String> {\n    let mut captures = Vec::new();\n",
    );
    // Zero identifies no scenario in generated native-capture IDs; the first
    // scenario therefore starts at one, which feeds every deterministic trace
    // and span identifier below.
    for (scenario_position, scenario) in plan.scenarios.iter().enumerate() {
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
            "            scenario_name: {}.to_string(),\n            trace_id: specgate_runtime::capture::TraceId::try_from({}).map_err(|error| error.to_string())?,\n            run_span_id: specgate_runtime::capture::SpanId::try_from({}).map_err(|error| error.to_string())?,\n            scenario_span_id: specgate_runtime::capture::SpanId::try_from({}).map_err(|error| error.to_string())?,\n        }})\n        .start_time({start_time})\n        .clock_step({CLOCK_STEP})\n        .build().map_err(|error| error.to_string())?,\n    ).map_err(|error| error.to_string())?;\n",
            string_literal(&scenario.name),
            string_literal(&trace_id),
            string_literal(&run_span_id),
            string_literal(&scenario_span_id),
        )
        .expect("writing to a String cannot fail");
        for operation in &scenario.operations {
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
            let mut path = String::from("candidate");
            for segment in &link.module_path {
                path.push_str("::");
                path.push_str(segment);
            }
            path.push_str("::");
            path.push_str(&link.fn_name);
            let arguments = operation
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
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(source, "    let _ = {path}({arguments});").expect("writing to a String cannot fail");
        }
        source.push_str("    captures.push(specgate_runtime::capture::finish().map_err(|error| error.to_string())?);\n");
    }
    source.push_str(
        "    let json = serde_json::to_vec(&captures).map_err(|error| format!(\"failed to serialize native captures: {error}\"))?;\n    publish(sidecar, &json).map_err(|error| format!(\"failed to write native capture sidecar: {error}\"))?;\n    Ok(())\n}\n\nfn main() -> Result<(), String> {\n    let sidecar = std::env::args_os().nth(1).ok_or_else(|| \"missing capture sidecar path\".to_string())?;\n    run(std::path::Path::new(&sidecar), |path, bytes| std::fs::write(path, bytes))\n}\n",
    );
    source
}

pub(super) fn render_value(value: &ReplayValue, rust_type: impl AsRef<str>) -> String {
    let rust_type = rust_type.as_ref();
    let native = rust_type.chars().filter(|character| !character.is_whitespace()).collect::<String>();
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
