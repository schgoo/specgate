//! Replay decoding and planning integration tests.

use serde_json::json;
use sha2::{Digest, Sha256};
use specgate::{ComponentId, OperationName};
use specgate_ctsc::{
    registry::encode as encode_typed_registry,
    replay::{decode as decode_replay_bundle_result, model::Value as ReplayValue},
    validation::{BundleBytes, DocumentBytes, bytes, validate_bundle},
};
use specgate_runtime::capture::{Capture, Config, ConfigDeps, begin_operation, finish, start};
use specgate_runtime::value::Value;

fn encode_schema_registry_result(
    id: impl Into<specgate_ctsc::registry::Id>,
    version: impl Into<specgate_ctsc::registry::Version>,
    schema: impl AsRef<str>,
) -> specgate_ctsc::registry::error::Result<specgate_ctsc::registry::Encoding> {
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
) -> Result<specgate_ctsc::capture::Encoding, specgate_ctsc::capture::error::Error> {
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
) -> Result<specgate_ctsc::capture::Encoding, specgate_ctsc::capture::error::Error> {
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

fn encode_replayed_native_captures_otlp_result(
    captures: &[Capture],
    tool_version: &str,
    target_name: &str,
    target_language: &str,
    registry_id: &str,
    registry_version: &str,
    registry_digest: &str,
) -> Result<specgate_ctsc::capture::Encoding, specgate_ctsc::capture::error::Error> {
    encode_capture(
        captures,
        tool_version,
        target_name,
        target_language,
        registry_id,
        registry_version,
        registry_digest,
        true,
    )
}

const REGISTRY_ID: &str = "urn:ctsc:registry:fixture.replay";
const REGISTRY_VERSION: &str = "0.1.0";

#[test]
fn named_type_owner_preserves_ctsc_wire_fields_without_leaking_identifier_storage() {
    let encoded = json!({
        "kind": "named",
        "name": "Money",
        "componentId": "example.types",
        "registryId": "urn:ctsc:registry:types"
    });
    let value: specgate_ctsc::replay::model::Type = serde_json::from_value(encoded.clone()).expect("decode named type");
    let (name, owner) = value.named_parts().expect("expected named type");
    assert_eq!(name, "Money");
    assert_eq!(owner.component_id(), Some("example.types"));
    assert_eq!(owner.registry_id(), Some("urn:ctsc:registry:types"));
    assert_eq!(serde_json::to_value(value).expect("encode named type"), encoded);
}

#[test]
fn decodes_linked_top_level_stimuli_and_ignores_nested_instructions() {
    let registry = make_registry(
        r#"{"component":"fixture.replay","operations":[
            {"name":"outer","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"},
            {"name":"inner","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"}
        ],"types":[]}"#,
    );
    let capture = native_capture(true);
    let trace = reference_trace(&capture, &registry);
    let manifest = make_manifest(&registry, trace.as_bytes(), &["scenario"]);

    let decoded = decode_replay_bundle_result(&manifest, &registry, trace.as_bytes()).unwrap();

    assert_eq!(decoded.component_id, "fixture.replay");
    assert_eq!(decoded.scenarios.len(), 1);
    assert_eq!(decoded.scenarios[0].operations.len(), 1);
    let operation = &decoded.scenarios[0].operations[0];
    assert_eq!(operation.operation_name, "outer");
    assert_eq!(operation.inputs[0].name, "value");
    assert_eq!(operation.inputs[0].value, ReplayValue::I32(2));
}

#[test]
fn rejects_digest_linkage_empty_scenario_and_structured_values() {
    let registry = make_registry(
        r#"{"component":"fixture.replay","operations":[
            {"name":"outer","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"},
            {"name":"inner","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"}
        ],"types":[]}"#,
    );
    let capture = native_capture(false);
    let trace = reference_trace(&capture, &registry);
    let manifest = make_manifest(&registry, trace.as_bytes(), &["scenario"]);

    let mut wrong_version: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
    wrong_version["formatVersion"] = json!("9.9.9");
    assert!(
        decode_replay_bundle_result(serde_json::to_vec(&wrong_version).unwrap(), &registry, trace.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("unsupported capture manifest format/version")
    );

    let mut corrupt_registry = registry.clone();
    corrupt_registry.push(b' ');
    assert!(
        decode_replay_bundle_result(&manifest, &corrupt_registry, trace.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("registry digest mismatch")
    );

    let mut unlinked: serde_json::Value = serde_json::from_str(&trace).unwrap();
    let attributes = unlinked["resourceSpans"][0]["resource"]["attributes"].as_array_mut().unwrap();
    let registry_id = attributes
        .iter_mut()
        .find(|attribute| attribute["key"] == "conformance.registry.id")
        .unwrap();
    registry_id["value"]["stringValue"] = json!("urn:ctsc:registry:wrong");
    let unlinked = serde_json::to_vec(&unlinked).unwrap();
    let unlinked_manifest = make_manifest(&registry, &unlinked, &["scenario"]);
    assert!(
        decode_replay_bundle_result(&unlinked_manifest, &registry, &unlinked)
            .unwrap_err()
            .to_string()
            .contains("conformance.registry.id")
    );

    let mut overflowing: serde_json::Value = serde_json::from_str(&trace).unwrap();
    let operation = overflowing["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|span| span["name"] == "conformance.operation")
        .unwrap();
    let inputs = operation["attributes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|attribute| attribute["key"] == "conformance.operation.inputs")
        .unwrap();
    let value = inputs["value"]["kvlistValue"]["values"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|input| input["key"] == "value")
        .unwrap();
    value["value"]["intValue"] = json!("2147483648");
    let overflowing = serde_json::to_vec(&overflowing).unwrap();
    let overflowing_manifest = make_manifest(&registry, &overflowing, &["scenario"]);
    assert!(
        decode_replay_bundle_result(&overflowing_manifest, &registry, &overflowing)
            .unwrap_err()
            .to_string()
            .contains("outside supported range")
    );

    let empty_capture = empty_native_capture();
    let empty_trace = reference_trace(&empty_capture, &registry);
    let empty_manifest = make_manifest(&registry, empty_trace.as_bytes(), &["empty"]);
    assert!(
        decode_replay_bundle_result(&empty_manifest, &registry, empty_trace.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("contains no top-level operations")
    );

    let structured_registry = make_registry(
        r#"{"component":"fixture.replay","operations":[
            {"name":"items","is_async":false,"inputs":[{"name":"values","ty":"List<i32>"}],"output":"i32"}
        ],"types":[]}"#,
    );
    let structured_capture = structured_native_capture();
    let structured_trace = reference_trace(&structured_capture, &structured_registry);
    let structured_manifest = make_manifest(&structured_registry, structured_trace.as_bytes(), &["structured"]);
    assert!(
        decode_replay_bundle_result(&structured_manifest, &structured_registry, structured_trace.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("unsupported structured replay type")
    );

    let bundle_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("rust root")
        .join("target")
        .join(format!("structured-bundle-{}", std::process::id()));
    std::fs::create_dir_all(&bundle_dir).unwrap();
    std::fs::write(bundle_dir.join("manifest.json"), &structured_manifest).unwrap();
    std::fs::write(bundle_dir.join("registry.ctsc.json"), &structured_registry).unwrap();
    std::fs::write(bundle_dir.join("reference.otlp.json"), structured_trace.as_bytes()).unwrap();
    let report = validate_bundle(&bundle_dir);
    let manifest_path = bundle_dir.join("manifest.json");
    let registry_path = bundle_dir.join("registry.ctsc.json");
    let trace_path = bundle_dir.join("reference.otlp.json");
    let bytes_report = bytes::validate_bundle(
        &bundle_dir,
        BundleBytes::new(
            DocumentBytes::new(&manifest_path, &structured_manifest),
            DocumentBytes::new(&registry_path, &structured_registry),
            DocumentBytes::new(&trace_path, structured_trace.as_bytes()),
        ),
    );
    std::fs::remove_dir_all(&bundle_dir).unwrap();
    assert_eq!(bytes_report, report);
    assert!(
        report.valid,
        "bundle validation must not inherit replay's structured-input restriction: {:#?}",
        report.issues
    );
}

#[test]
fn replay_encoder_uses_independent_deterministic_ids() {
    let registry = make_registry(
        r#"{"component":"fixture.replay","operations":[
            {"name":"outer","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"},
            {"name":"inner","is_async":false,"inputs":[{"name":"value","ty":"i32"}],"output":"i32"}
        ],"types":[]}"#,
    );
    let capture = native_capture(false);
    let digest = digest(&registry);
    let reference = encode_native_captures_otlp_result(
        std::slice::from_ref(&capture),
        "0.6.0",
        "reference",
        "rust",
        REGISTRY_ID,
        REGISTRY_VERSION,
        &digest,
    )
    .unwrap();
    let first = encode_replayed_native_captures_otlp_result(
        std::slice::from_ref(&capture),
        "0.6.0",
        "candidate",
        "rust",
        REGISTRY_ID,
        REGISTRY_VERSION,
        &digest,
    )
    .unwrap();
    let second =
        encode_replayed_native_captures_otlp_result(&[capture], "0.6.0", "candidate", "rust", REGISTRY_ID, REGISTRY_VERSION, &digest)
            .unwrap();

    assert_eq!(first, second);
    assert_ne!(first.otlp_json, reference.otlp_json);
    assert!(first.otlp_json.contains("\"traceId\":\"00000000000000000000000000000002\""));
    assert!(first.otlp_json.contains("\"conformance.run.id\""));
    assert!(first.otlp_json.contains("\"specgate.replay\""));
}

fn make_registry(schema: &str) -> Vec<u8> {
    encode_schema_registry_result(REGISTRY_ID.to_string(), REGISTRY_VERSION.to_string(), schema)
        .unwrap()
        .registry_json
        .into_bytes()
}

fn reference_trace(capture: &Capture, registry: &[u8]) -> String {
    encode_native_captures_otlp_result(
        std::slice::from_ref(capture),
        "0.6.0",
        "reference",
        "rust",
        REGISTRY_ID,
        REGISTRY_VERSION,
        &digest(registry),
    )
    .unwrap()
    .otlp_json
}

fn make_manifest(registry: &[u8], trace: &[u8], names: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format": "specgate.capture-manifest",
        "formatVersion": "0.1.0",
        "componentId": "fixture.replay",
        "target": {"name": "reference", "language": "rust"},
        "tool": {"name": "specgate", "version": "0.6.0"},
        "registry": {
            "path": "registry.ctsc.json",
            "id": REGISTRY_ID,
            "version": REGISTRY_VERSION,
            "digest": digest(registry)
        },
        "reference": {
            "path": "reference.otlp.json",
            "digest": digest(trace)
        },
        "scenarios": {"count": names.len(), "names": names}
    }))
    .unwrap()
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn config(name: &str) -> Config {
    Config::builder(ConfigDeps {
        scenario_name: name.into(),
        trace_id: "33333333333333333333333333333333".try_into().unwrap(),
        run_span_id: "3333333333333301".try_into().unwrap(),
        scenario_span_id: "3333333333333302".try_into().unwrap(),
    })
    .build()
    .unwrap()
}

fn native_capture(nested: bool) -> Capture {
    start(config("scenario")).unwrap();
    let mut outer = begin_operation(ComponentId::from("fixture.replay"), OperationName::from("outer")).unwrap();
    outer.input("value", Value::Integer(2)).unwrap();
    if nested {
        let mut inner = begin_operation(ComponentId::from("fixture.replay"), OperationName::from("inner")).unwrap();
        inner.input("value", Value::Integer(3)).unwrap();
        inner.result(Value::Integer(6)).unwrap();
    }
    outer.result(Value::Integer(5)).unwrap();
    finish().unwrap()
}

fn empty_native_capture() -> Capture {
    start(config("empty")).unwrap();
    finish().unwrap()
}

fn structured_native_capture() -> Capture {
    start(config("structured")).unwrap();
    let mut operation = begin_operation(ComponentId::from("fixture.replay"), OperationName::from("items")).unwrap();
    operation
        .input("values", Value::List(vec![Value::Integer(1), Value::Integer(2)]))
        .unwrap();
    operation.result(Value::Integer(3)).unwrap();
    finish().unwrap()
}
