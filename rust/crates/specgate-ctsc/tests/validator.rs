use serde_json::json;
use sha2::{Digest, Sha256};
use specgate::{SpecEvent, spec_component, spec_operation};
use specgate_ctsc::{encode_native_captures_otlp_result, encode_schema_registry_result};
use specgate_discovery::discovery::{Registry, normalize_registry};
use specgate_runtime::{NativeCaptureConfig, Value, begin_native_operation, finish_native_capture, start_native_capture};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

spec_component!("fixture.tuple_variants");

#[derive(SpecEvent)]
enum TupleVariant {
    Single(i32),
    Multiple(i32, String),
}

#[derive(SpecEvent)]
#[spec_component("fixture.macro_hygiene")]
enum HygienicVariant {
    Named {
        payload: i32,
        __specgate_variant_0_payload: String,
        field_0: bool,
        __specgate_variant_0_field_3: i64,
    },
    Tuple(i32, String),
}

#[spec_operation("emit_tuple_variant")]
fn emit_tuple_variant(multiple: bool) -> TupleVariant {
    if multiple {
        TupleVariant::Multiple(7, "seven".to_string())
    } else {
        TupleVariant::Single(1)
    }
}

#[spec_operation("emit_hygienic_variant", spec = "fixture.macro_hygiene")]
fn emit_hygienic_variant(named: bool) -> HygienicVariant {
    if named {
        HygienicVariant::Named {
            payload: 7,
            __specgate_variant_0_payload: "seven".to_string(),
            field_0: true,
            __specgate_variant_0_field_3: 9,
        }
    } else {
        HygienicVariant::Tuple(11, "eleven".to_string())
    }
}

#[test]
fn native_generated_otlp_passes_ctsc_trace_validator_when_available() {
    let capture = native_capture("nested_double", Some("alice"));
    let result = encode_native_captures_otlp_result(
        &[capture],
        "0.6.0",
        "rust-reference",
        "rust",
        "urn:ctsc:registry:test",
        "1.0.0",
        &format!("sha256:{}", "a".repeat(64)),
    )
    .unwrap();

    validate_generated_document("trace", "native-trace", &result.otlp_json);
}

#[test]
fn generated_optional_traces_match_linked_registry_when_available() {
    let registry_id = "urn:ctsc:registry:fixture.native-capture:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        r#"{"component":"fixture.native_capture","dependencies":[],"operations":[{"name":"echo_optional","is_async":false,"inputs":[{"name":"value","ty":"Option<string>"}],"output":"string","empty":true,"errors":[],"setups":[]}],"types":[]}"#,
    )
    .expect("optional schema should encode");

    let digest = format!("sha256:{:x}", Sha256::digest(registry.registry_json.as_bytes()));
    for (label, value) in [("optional-some", Some("alice")), ("optional-none", None)] {
        let result = encode_native_captures_otlp_result(
            &[native_capture(label, value)],
            "0.6.0",
            "rust-reference",
            "rust",
            registry_id,
            registry_version,
            &digest,
        )
        .unwrap();
        validate_generated_linked_documents(label, &result.otlp_json, &registry.registry_json);
    }
}

#[test]
fn generated_schema_registry_passes_ctsc_registry_validator_when_available() {
    let result = encode_schema_registry_result(
        "urn:ctsc:registry:fixture.rich:1".to_string(),
        "1.0.0".to_string(),
        r#"{
            "component":"fixture.rich",
            "operations":[{
                "name":"transform",
                "is_async":false,
                "inputs":[
                    {"name":"person","ty":"Person"},
                    {"name":"points","ty":"List<Point>"},
                    {"name":"fallback","ty":"Option<Point>"}
                ],
                "output":"Shape",
                "empty":false,
                "errors":[],
                "setups":[]
            }],
            "dependencies":[],
            "types":[
                {"name":"Person","kind":"struct","fields":[{"name":"name","ty":"string"},{"name":"location","ty":"Point"}],"variants":[]},
                {"name":"Point","kind":"struct","fields":[{"name":"x","ty":"i32"},{"name":"y","ty":"i32"}],"variants":[]},
                {"name":"Shape","kind":"enum","fields":[],"variants":[
                    {"name":"Circle","fields":[{"name":"radius","ty":"i32"}]},
                    {"name":"Point","fields":[]}
                ]}
            ]
        }"#,
    )
    .expect("normalized schema should encode");

    validate_generated_document("registry", "schema-registry", &result.registry_json);
}

#[test]
fn dependency_owned_named_types_are_component_qualified() {
    let result = encode_schema_registry_result(
        "urn:ctsc:registry:fixture.app:1".to_string(),
        "1.0.0".to_string(),
        r#"{
            "component":"fixture.app",
            "dependencies":["fixture.shared"],
            "dependency_types":[{
                "component":"fixture.shared",
                "types":[{
                    "name":"Shared",
                    "kind":"struct",
                    "fields":[{"name":"id","ty":"i32"}],
                    "variants":[]
                }]
            }],
            "operations":[{
                "name":"store",
                "is_async":false,
                "inputs":[{"name":"value","ty":"fixture.shared::Shared"}],
                "output":"",
                "empty":false,
                "errors":[],
                "setups":[]
            }],
            "types":[]
        }"#,
    )
    .expect("cross-component schema should encode");

    let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
    assert_eq!(
        document["components"][0]["operations"][0]["inputs"][0]["type"],
        serde_json::json!({
            "kind": "named",
            "name": "Shared",
            "componentId": "fixture.shared"
        })
    );
    assert_eq!(document["components"][1]["id"], "fixture.shared");
    validate_generated_document("registry", "cross-component-registry", &result.registry_json);
}

#[test]
fn transitive_dependency_components_retain_their_dependency_lists() {
    let result = encode_schema_registry_result(
        "urn:ctsc:registry:fixture.transitive:1".to_string(),
        "1.0.0".to_string(),
        r#"{
            "component":"fixture.app",
            "dependencies":["fixture.middle"],
            "dependency_types":[
                {
                    "component":"fixture.middle",
                    "dependencies":["fixture.leaf"],
                    "types":[{
                        "name":"Middle",
                        "kind":"struct",
                        "fields":[{"name":"leaf","ty":"fixture.leaf::Leaf"}],
                        "variants":[]
                    }]
                },
                {
                    "component":"fixture.leaf",
                    "dependencies":[],
                    "types":[{
                        "name":"Leaf",
                        "kind":"struct",
                        "fields":[{"name":"id","ty":"i32"}],
                        "variants":[]
                    }]
                }
            ],
            "operations":[{
                "name":"store",
                "is_async":false,
                "inputs":[{"name":"value","ty":"fixture.middle::Middle"}],
                "output":"",
                "empty":false,
                "errors":[],
                "setups":[]
            }],
            "types":[]
        }"#,
    )
    .expect("transitive dependency schema should encode");

    let document: serde_json::Value = serde_json::from_str(&result.registry_json).unwrap();
    let middle = document["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|component| component["id"] == "fixture.middle")
        .unwrap();
    assert_eq!(middle["dependencies"], serde_json::json!([{"componentId":"fixture.leaf"}]));
    assert_eq!(
        middle["types"][0]["fields"][0]["type"],
        serde_json::json!({"kind":"named","name":"Leaf","componentId":"fixture.leaf"})
    );
    validate_generated_document("registry", "transitive-component-registry", &result.registry_json);
}

#[test]
fn registry_encoder_rejects_dependency_cycles() {
    let error = encode_schema_registry_result(
        "urn:ctsc:registry:cycle:1".to_string(),
        "1.0.0".to_string(),
        r#"{
            "component":"fixture.a",
            "dependencies":["fixture.b"],
            "dependency_types":[{
                "component":"fixture.b",
                "dependencies":["fixture.a"],
                "types":[]
            }],
            "operations":[{
                "name":"run","is_async":false,"inputs":[],"output":"",
                "empty":false,"errors":[],"setups":[]
            }],
            "types":[]
        }"#,
    )
    .unwrap_err();
    assert!(error.contains("fixture.a -> fixture.b -> fixture.a"));
}

#[test]
fn unit_result_and_optional_unit_traces_pass_linked_validation() {
    let registry_id = "urn:ctsc:registry:fixture.unit:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        r#"{
            "component":"fixture.unit",
            "dependencies":[],
            "dependency_types":[],
            "operations":[
                {
                    "name":"fallible",
                    "is_async":false,
                    "inputs":[{"name":"fail","ty":"bool"}],
                    "output":"",
                    "empty":false,
                    "errors":[{"name":"error","ty":"string"}],
                    "setups":[]
                },
                {
                    "name":"optional_unit",
                    "is_async":false,
                    "inputs":[{"name":"present","ty":"bool"}],
                    "output":"Option<unit>",
                    "empty":false,
                    "errors":[],
                    "setups":[]
                },
                {
                    "name":"unit_error",
                    "is_async":false,
                    "inputs":[{"name":"fail","ty":"bool"}],
                    "output":"i32",
                    "empty":false,
                    "errors":[{"name":"error","ty":"unit"}],
                    "setups":[]
                }
            ],
            "types":[]
        }"#,
    )
    .unwrap();
    let digest = format!("sha256:{:x}", Sha256::digest(registry.registry_json.as_bytes()));
    let captures = [
        unit_result_capture("unit-ok", false),
        unit_result_capture("unit-error", true),
        optional_unit_capture("optional-unit-some", true),
        optional_unit_capture("optional-unit-none", false),
        unit_error_capture("unit-error", true),
    ];
    let trace =
        encode_native_captures_otlp_result(&captures, "0.6.0", "rust-reference", "rust", registry_id, registry_version, &digest).unwrap();
    validate_generated_linked_documents("unit-results", &trace.otlp_json, &registry.registry_json);
}

#[test]
fn annotated_tuple_variants_pass_linked_validation() {
    let raw_registry = Registry::parse(&specgate::__rt::discovery_json()).unwrap();
    let schema = normalize_registry(&raw_registry, "rust", "fixture.tuple_variants").unwrap();
    let registry_id = "urn:ctsc:registry:fixture.tuple-variants:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        &serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();
    let document: serde_json::Value = serde_json::from_str(&registry.registry_json).unwrap();
    let variants = document["components"][0]["types"][0]["variants"].as_array().unwrap();
    assert_eq!(
        variants[0]["payload"],
        json!({"kind":"tuple","items":[{"kind":"primitive","name":"i32"}]})
    );
    assert_eq!(
        variants[1]["payload"],
        json!({"kind":"tuple","items":[{"kind":"primitive","name":"i32"},{"kind":"primitive","name":"string"}]})
    );

    let captures = [
        tuple_variant_capture("tuple-single", false),
        tuple_variant_capture("tuple-multiple", true),
    ];
    let digest = format!("sha256:{:x}", Sha256::digest(registry.registry_json.as_bytes()));
    let trace =
        encode_native_captures_otlp_result(&captures, "0.6.0", "rust-reference", "rust", registry_id, registry_version, &digest).unwrap();
    validate_generated_linked_documents("tuple-variants", &trace.otlp_json, &registry.registry_json);
}

#[test]
fn annotated_enum_projection_is_hygienic_and_passes_linked_validation() {
    let named = emit_hygienic_variant(true);
    assert_eq!(
        specgate::__rt::ToNativeValue::to_native_value(&named),
        Value::Map(std::collections::BTreeMap::from([(
            "Named".to_string(),
            Value::Map(std::collections::BTreeMap::from([
                ("__specgate_variant_0_field_3".to_string(), Value::Integer(9)),
                ("__specgate_variant_0_payload".to_string(), Value::String("seven".to_string()),),
                ("field_0".to_string(), Value::Bool(true)),
                ("payload".to_string(), Value::Integer(7)),
            ])),
        )]))
    );
    let tuple = emit_hygienic_variant(false);
    assert_eq!(
        specgate::__rt::ToNativeValue::to_native_value(&tuple),
        Value::Map(std::collections::BTreeMap::from([(
            "Tuple".to_string(),
            Value::List(vec![Value::Integer(11), Value::String("eleven".to_string())]),
        )]))
    );

    let raw_registry = Registry::parse(&specgate::__rt::discovery_json()).unwrap();
    let schema = normalize_registry(&raw_registry, "rust", "fixture.macro_hygiene").unwrap();
    let registry_id = "urn:ctsc:registry:fixture.macro-hygiene:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        &serde_json::to_string(&schema).unwrap(),
    )
    .unwrap();
    let document: serde_json::Value = serde_json::from_str(&registry.registry_json).unwrap();
    let variants = document["components"][0]["types"][0]["variants"].as_array().unwrap();
    assert_eq!(
        variants[0]["payload"],
        json!({
            "kind":"record",
            "fields":[
                {"name":"payload","type":{"kind":"primitive","name":"i32"}},
                {"name":"__specgate_variant_0_payload","type":{"kind":"primitive","name":"string"}},
                {"name":"field_0","type":{"kind":"primitive","name":"bool"}},
                {"name":"__specgate_variant_0_field_3","type":{"kind":"primitive","name":"i64"}}
            ]
        })
    );
    assert_eq!(
        variants[1]["payload"],
        json!({"kind":"tuple","items":[{"kind":"primitive","name":"i32"},{"kind":"primitive","name":"string"}]})
    );

    let captures = [
        hygienic_variant_capture("hygienic-named", true),
        hygienic_variant_capture("hygienic-tuple", false),
    ];
    let digest = format!("sha256:{:x}", Sha256::digest(registry.registry_json.as_bytes()));
    let trace =
        encode_native_captures_otlp_result(&captures, "0.6.0", "rust-reference", "rust", registry_id, registry_version, &digest).unwrap();
    validate_generated_linked_documents("macro-hygiene", &trace.otlp_json, &registry.registry_json);
}

fn validate_generated_document(kind: &str, file_label: &str, json: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.ancestors().nth(3).expect("failed to derive repository root");
    let output_dir = repo_root.join("rust").join("target");
    fs::create_dir_all(&output_dir).expect("failed to create validator output directory");
    let document_path = output_dir.join(format!("specgate-ctsc-validator-{file_label}-{}.json", std::process::id()));
    fs::write(&document_path, json).expect("failed to write generated CTSC JSON");

    let validator = repo_root.join("docs").join("ctsc").join("validate.py");
    if !validator.is_file() {
        let _ = fs::remove_file(&document_path);
        eprintln!("CTSC validator unavailable: {}", validator.display());
        return;
    }

    let output = invoke_python_validator(&validator, kind, &document_path);
    let _ = fs::remove_file(&document_path);

    let Some(output) = output else {
        eprintln!("CTSC validator unavailable: Python was not found");
        return;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && (stdout.contains("missing validator dependencies") || stderr.contains("missing validator dependencies"))
    {
        eprintln!("CTSC validator unavailable: {stdout}{stderr}");
        return;
    }

    assert!(
        output.status.success(),
        "CTSC validator rejected generated {kind}:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

fn native_capture(label: &str, value: Option<&str>) -> specgate_runtime::NativeCapture {
    start_native_capture(NativeCaptureConfig {
        scenario_name: label.to_string(),
        trace_id: "44444444444444444444444444444444".to_string(),
        run_span_id: "4444444444444401".to_string(),
        scenario_span_id: "4444444444444402".to_string(),
        operation_span_ids: vec!["4444444444444403".to_string()],
        start_time_unix_nano: 4_000_000_000,
        clock_step_unix_nano: 100,
    })
    .unwrap();
    let mut operation = begin_native_operation("fixture.native_capture", "echo_optional").unwrap();
    let input = match value {
        Some(value) => Value::Map(std::collections::BTreeMap::from([(
            "Some".to_string(),
            Value::String(value.to_string()),
        )])),
        None => Value::Map(std::collections::BTreeMap::from([(
            "None".to_string(),
            Value::Map(std::collections::BTreeMap::new()),
        )])),
    };
    operation.record_input("value", input).unwrap();
    match value {
        Some(value) => operation.complete_result(Value::String(value.to_string())).unwrap(),
        None => operation.complete_empty().unwrap(),
    }
    finish_native_capture().unwrap()
}

fn unit_result_capture(label: &str, fail: bool) -> specgate_runtime::NativeCapture {
    start_native_capture(capture_config(label)).unwrap();
    let mut operation = begin_native_operation("fixture.unit", "fallible").unwrap();
    operation.record_input("fail", Value::Bool(fail)).unwrap();
    if fail {
        operation.complete_error("error", Value::String("failed".to_string())).unwrap();
    } else {
        operation.complete_unit().unwrap();
    }
    finish_native_capture().unwrap()
}

fn optional_unit_capture(label: &str, present: bool) -> specgate_runtime::NativeCapture {
    start_native_capture(capture_config(label)).unwrap();
    let mut operation = begin_native_operation("fixture.unit", "optional_unit").unwrap();
    operation.record_input("present", Value::Bool(present)).unwrap();
    let variant = if present { "Some" } else { "None" };
    operation
        .complete_result(Value::Map(std::collections::BTreeMap::from([(
            variant.to_string(),
            Value::Map(std::collections::BTreeMap::new()),
        )])))
        .unwrap();
    finish_native_capture().unwrap()
}

fn unit_error_capture(label: &str, fail: bool) -> specgate_runtime::NativeCapture {
    start_native_capture(capture_config(label)).unwrap();
    let mut operation = begin_native_operation("fixture.unit", "unit_error").unwrap();
    operation.record_input("fail", Value::Bool(fail)).unwrap();
    if fail {
        operation.complete_error_unit("error").unwrap();
    } else {
        operation.complete_result(Value::Integer(7)).unwrap();
    }
    finish_native_capture().unwrap()
}

fn capture_config(label: &str) -> NativeCaptureConfig {
    NativeCaptureConfig {
        scenario_name: label.to_string(),
        trace_id: "88888888888888888888888888888888".to_string(),
        run_span_id: "8888888888888801".to_string(),
        scenario_span_id: "8888888888888802".to_string(),
        operation_span_ids: vec!["8888888888888803".to_string()],
        start_time_unix_nano: 5_000_000_000,
        clock_step_unix_nano: 100,
    }
}

fn tuple_variant_capture(label: &str, multiple: bool) -> specgate_runtime::NativeCapture {
    start_native_capture(capture_config(label)).unwrap();
    let _ = emit_tuple_variant(multiple);
    finish_native_capture().unwrap()
}

fn hygienic_variant_capture(label: &str, named: bool) -> specgate_runtime::NativeCapture {
    start_native_capture(capture_config(label)).unwrap();
    let _ = emit_hygienic_variant(named);
    finish_native_capture().unwrap()
}

fn validate_generated_linked_documents(file_label: &str, trace_json: &str, registry_json: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.ancestors().nth(3).expect("failed to derive repository root");
    let output_dir = repo_root.join("rust").join("target");
    fs::create_dir_all(&output_dir).expect("failed to create validator output directory");
    let trace_path = output_dir.join(format!("specgate-ctsc-validator-{file_label}-trace-{}.json", std::process::id()));
    let registry_path = output_dir.join(format!("specgate-ctsc-validator-{file_label}-registry-{}.json", std::process::id()));
    fs::write(&registry_path, registry_json).expect("failed to write generated CTSC registry");

    fs::write(&trace_path, trace_json).expect("failed to write generated CTSC trace");

    let validator = repo_root.join("docs").join("ctsc").join("validate.py");
    if !validator.is_file() {
        let _ = fs::remove_file(&trace_path);
        let _ = fs::remove_file(&registry_path);
        eprintln!("CTSC validator unavailable: {}", validator.display());
        return;
    }

    let output = invoke_python_linked_validator(&validator, &trace_path, &registry_path);
    let _ = fs::remove_file(&trace_path);
    let _ = fs::remove_file(&registry_path);
    assert_validator_output(output, "linked");
}

fn assert_validator_output(output: Option<Output>, kind: &str) {
    let Some(output) = output else {
        eprintln!("CTSC validator unavailable: Python was not found");
        return;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() && (stdout.contains("missing validator dependencies") || stderr.contains("missing validator dependencies"))
    {
        eprintln!("CTSC validator unavailable: {stdout}{stderr}");
        return;
    }

    assert!(
        output.status.success(),
        "CTSC validator rejected generated {kind}:\nstdout: {stdout}\nstderr: {stderr}"
    );
}

fn invoke_python_validator(validator: &Path, kind: &str, document_path: &Path) -> Option<Output> {
    let candidates: [(&str, &[&str]); 2] = [("python", &[]), ("py", &["-3"])];

    for (program, prefix_args) in candidates {
        let output = Command::new(program)
            .args(prefix_args)
            .arg(validator)
            .arg(kind)
            .arg(document_path)
            .output();

        match output {
            Ok(output) if python_launcher_is_unavailable(&output) => {}
            Ok(output) => return Some(output),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to invoke CTSC validator with {program}: {error}"),
        }
    }

    None
}

fn invoke_python_linked_validator(validator: &Path, trace_path: &Path, registry_path: &Path) -> Option<Output> {
    let candidates: [(&str, &[&str]); 2] = [("python", &[]), ("py", &["-3"])];

    for (program, prefix_args) in candidates {
        let output = Command::new(program)
            .args(prefix_args)
            .arg(validator)
            .arg("linked")
            .arg(trace_path)
            .arg(registry_path)
            .output();

        match output {
            Ok(output) if python_launcher_is_unavailable(&output) => {}
            Ok(output) => return Some(output),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("failed to invoke CTSC validator with {program}: {error}"),
        }
    }

    None
}

fn python_launcher_is_unavailable(output: &Output) -> bool {
    if output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    stdout.contains("Python was not found")
        || stderr.contains("Python was not found")
        || stdout.contains("No suitable Python runtime found")
        || stderr.contains("No suitable Python runtime found")
}
