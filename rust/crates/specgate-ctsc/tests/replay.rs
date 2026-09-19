use serde_json::json;
use sha2::{Digest, Sha256};
use specgate_ctsc::{
    ReplayValue, decode_replay_bundle_result, encode_native_captures_otlp_result, encode_replayed_native_captures_otlp_result,
    encode_schema_registry_result,
};
use specgate_runtime::{NativeCapture, NativeCaptureConfig, Value, begin_native_operation, finish_native_capture, start_native_capture};

const REGISTRY_ID: &str = "urn:ctsc:registry:fixture.replay";
const REGISTRY_VERSION: &str = "0.1.0";

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
        decode_replay_bundle_result(&serde_json::to_vec(&wrong_version).unwrap(), &registry, trace.as_bytes())
            .unwrap_err()
            .contains("unsupported capture manifest format/version")
    );

    let mut corrupt_registry = registry.clone();
    corrupt_registry.push(b' ');
    assert!(
        decode_replay_bundle_result(&manifest, &corrupt_registry, trace.as_bytes())
            .unwrap_err()
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
            .contains("outside supported range")
    );

    let empty_capture = empty_native_capture();
    let empty_trace = reference_trace(&empty_capture, &registry);
    let empty_manifest = make_manifest(&registry, empty_trace.as_bytes(), &["empty"]);
    assert!(
        decode_replay_bundle_result(&empty_manifest, &registry, empty_trace.as_bytes())
            .unwrap_err()
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
            .contains("unsupported structured replay type")
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

fn reference_trace(capture: &NativeCapture, registry: &[u8]) -> String {
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

fn config(name: &str) -> NativeCaptureConfig {
    NativeCaptureConfig {
        scenario_name: name.to_string(),
        trace_id: "33333333333333333333333333333333".to_string(),
        run_span_id: "3333333333333301".to_string(),
        scenario_span_id: "3333333333333302".to_string(),
        operation_span_ids: Vec::new(),
        start_time_unix_nano: 0,
        clock_step_unix_nano: 1,
    }
}

fn native_capture(nested: bool) -> NativeCapture {
    start_native_capture(config("scenario")).unwrap();
    let mut outer = begin_native_operation("fixture.replay", "outer").unwrap();
    outer.record_input("value", Value::Integer(2)).unwrap();
    if nested {
        let mut inner = begin_native_operation("fixture.replay", "inner").unwrap();
        inner.record_input("value", Value::Integer(3)).unwrap();
        inner.complete_result(Value::Integer(6)).unwrap();
    }
    outer.complete_result(Value::Integer(5)).unwrap();
    finish_native_capture().unwrap()
}

fn empty_native_capture() -> NativeCapture {
    start_native_capture(config("empty")).unwrap();
    finish_native_capture().unwrap()
}

fn structured_native_capture() -> NativeCapture {
    start_native_capture(config("structured")).unwrap();
    let mut operation = begin_native_operation("fixture.replay", "items").unwrap();
    operation
        .record_input("values", Value::List(vec![Value::Integer(1), Value::Integer(2)]))
        .unwrap();
    operation.complete_result(Value::Integer(3)).unwrap();
    finish_native_capture().unwrap()
}
