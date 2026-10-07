//! CTSC registry, trace, linked, and bundle validator integration tests.

use serde_json::json;
use sha2::{Digest, Sha256};
use specgate::{ComponentId, OperationName, SpecEvent, spec_component, spec_operation};
use specgate_ctsc::{
    registry::encode as encode_typed_registry,
    validation::{validate_linked, validate_registry, validate_trace},
};
use specgate_discovery::registry::Registry;
use specgate_discovery::schema::normalize_registry;
use specgate_runtime::capture::{Capture, Config, ConfigDeps, begin_operation, finish, start};
use specgate_runtime::value::Value;
use std::fs;
use std::path::Path;

fn encode_schema_registry_result(
    id: impl Into<specgate_ctsc::registry::Id>,
    version: impl Into<specgate_ctsc::registry::Version>,
    schema: impl AsRef<str>,
) -> specgate_ctsc::registry::Result<specgate_ctsc::registry::Encoding> {
    encode_typed_registry(id, version, specgate_ctsc::registry::Schema::new(schema.as_ref()))
}

#[expect(clippy::too_many_arguments, reason = "test helper mirrors all independent capture metadata inputs")]
fn encode_capture(
    captures: &[Capture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
    candidate: bool,
) -> Result<specgate_ctsc::capture::Encoding, specgate_ctsc::capture::Error> {
    let bytes = serde_json::to_vec(captures).expect("runtime captures serialize");
    let captures: Vec<specgate_ctsc::capture::Capture> = serde_json::from_slice(&bytes).expect("capture boundary matches runtime JSON");
    let metadata = specgate_ctsc::capture::Metadata::new(
        tool_version,
        specgate_ctsc::capture::Target::new(target_name, target_language),
        specgate_ctsc::capture::Registry::new(registry_id, registry_version, registry_digest),
    );
    if candidate {
        specgate_ctsc::capture::encode_candidate(&captures, &metadata)
    } else {
        specgate_ctsc::capture::encode_reference(&captures, &metadata)
    }
}

fn encode_native_captures_otlp_result(
    captures: &[Capture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<specgate_ctsc::capture::Encoding, specgate_ctsc::capture::Error> {
    encode_capture(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        false,
    )
}

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

/// Emit one tuple variant for native validation coverage.
#[spec_operation("emit_tuple_variant")]
pub fn emit_tuple_variant(multiple: bool) -> TupleVariant {
    if multiple {
        TupleVariant::Multiple(7, "seven".to_string())
    } else {
        TupleVariant::Single(1)
    }
}

/// Emit one macro-hygiene variant for native validation coverage.
#[spec_operation("emit_hygienic_variant", spec = "fixture.macro_hygiene")]
pub fn emit_hygienic_variant(named: bool) -> HygienicVariant {
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
fn native_generated_otlp_passes_native_ctsc_trace_validator() {
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
fn generated_optional_traces_match_native_linked_registry() {
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
fn generated_schema_registry_passes_native_ctsc_registry_validator() {
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
    assert!(error.to_string().contains("fixture.a -> fixture.b -> fixture.a"));
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
    let raw_registry = Registry::parse(specgate::__rt::discovery().to_string()).unwrap();
    let schema = normalize_registry(&raw_registry, specgate_discovery::binding::Language::Rust, "fixture.tuple_variants").unwrap();
    let registry_id = "urn:ctsc:registry:fixture.tuple-variants:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        serde_json::to_string(&schema).unwrap(),
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

    let raw_registry = Registry::parse(specgate::__rt::discovery().to_string()).unwrap();
    let schema = normalize_registry(&raw_registry, specgate_discovery::binding::Language::Rust, "fixture.macro_hygiene").unwrap();
    let registry_id = "urn:ctsc:registry:fixture.macro-hygiene:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        serde_json::to_string(&schema).unwrap(),
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

    let report = if kind == "registry" {
        validate_registry(&document_path, &[])
    } else {
        validate_trace(&document_path)
    };
    let _ = fs::remove_file(&document_path);
    assert!(
        report.valid,
        "native CTSC validator rejected generated {kind}: {:#?}",
        report.issues
    );
}

fn native_capture(label: &str, value: Option<&str>) -> Capture {
    start(
        Config::builder(ConfigDeps {
            scenario_name: label.into(),
            trace_id: "44444444444444444444444444444444".try_into().unwrap(),
            run_id: "4444444444444401".try_into().unwrap(),
            scenario_id: "4444444444444402".try_into().unwrap(),
        })
        .operation_ids(vec!["4444444444444403".try_into().unwrap()])
        .start_time(4_000_000_000)
        .clock_step(100)
        .build()
        .unwrap(),
    )
    .unwrap();
    let mut operation = begin_operation(ComponentId::from("fixture.native_capture"), OperationName::from("echo_optional")).unwrap();
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
    operation.input("value", input).unwrap();
    match value {
        Some(value) => operation.result(Value::String(value.to_string())).unwrap(),
        None => operation.empty().unwrap(),
    }
    finish().unwrap()
}

fn unit_result_capture(label: &str, fail: bool) -> Capture {
    start(capture_config(label)).unwrap();
    let mut operation = begin_operation(ComponentId::from("fixture.unit"), OperationName::from("fallible")).unwrap();
    operation.input("fail", Value::Bool(fail)).unwrap();
    if fail {
        operation.error("error", Value::String("failed".to_string())).unwrap();
    } else {
        operation.unit().unwrap();
    }
    finish().unwrap()
}

fn optional_unit_capture(label: &str, present: bool) -> Capture {
    start(capture_config(label)).unwrap();
    let mut operation = begin_operation(ComponentId::from("fixture.unit"), OperationName::from("optional_unit")).unwrap();
    operation.input("present", Value::Bool(present)).unwrap();
    let variant = if present { "Some" } else { "None" };
    operation
        .result(Value::Map(std::collections::BTreeMap::from([(
            variant.to_string(),
            Value::Map(std::collections::BTreeMap::new()),
        )])))
        .unwrap();
    finish().unwrap()
}

fn unit_error_capture(label: &str, fail: bool) -> Capture {
    start(capture_config(label)).unwrap();
    let mut operation = begin_operation(ComponentId::from("fixture.unit"), OperationName::from("unit_error")).unwrap();
    operation.input("fail", Value::Bool(fail)).unwrap();
    if fail {
        operation.error_unit("error").unwrap();
    } else {
        operation.result(Value::Integer(7)).unwrap();
    }
    finish().unwrap()
}

fn capture_config(label: &str) -> Config {
    Config::builder(ConfigDeps {
        scenario_name: label.into(),
        trace_id: "88888888888888888888888888888888".try_into().unwrap(),
        run_id: "8888888888888801".try_into().unwrap(),
        scenario_id: "8888888888888802".try_into().unwrap(),
    })
    .operation_ids(vec!["8888888888888803".try_into().unwrap()])
    .start_time(5_000_000_000)
    .clock_step(100)
    .build()
    .unwrap()
}

fn tuple_variant_capture(label: &str, multiple: bool) -> Capture {
    start(capture_config(label)).unwrap();
    let _ = emit_tuple_variant(multiple);
    finish().unwrap()
}

fn hygienic_variant_capture(label: &str, named: bool) -> Capture {
    start(capture_config(label)).unwrap();
    let _ = emit_hygienic_variant(named);
    finish().unwrap()
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

    let report = validate_linked(&trace_path, &registry_path, &[]);
    let _ = fs::remove_file(&trace_path);
    let _ = fs::remove_file(&registry_path);
    assert!(
        report.valid,
        "native CTSC validator rejected generated linked documents: {:#?}",
        report.issues
    );
}
