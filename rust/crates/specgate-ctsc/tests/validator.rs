use sha2::{Digest, Sha256};
use specgate_ctsc::{
    capture_native_optional_otlp, capture_native_rust_otlp, encode_discovery_registry, encode_legacy_trace_otlp,
    encode_schema_registry_result,
};
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[test]
fn generated_otlp_passes_ctsc_trace_validator_when_available() {
    let result = encode_legacy_trace_otlp(
        "add_2_3".to_string(),
        "fixture.stateless_add".to_string(),
        r#"[{"kind":"Run","operation":"add"},{"kind":"Event","name":"add.a","value":2},{"kind":"Event","name":"add.b","value":3},{"kind":"Event","name":"$result","value":5}]"#.to_string(),
        "11111111111111111111111111111111".to_string(),
        "1111111111111101".to_string(),
        "1111111111111102".to_string(),
        "1111111111111103".to_string(),
        "run-001".to_string(),
        1_000_000_000,
        "0.5.0".to_string(),
        "rust-reference".to_string(),
        "rust".to_string(),
    );

    validate_generated_document("trace", "trace", &result.otlp_json);
}

#[test]
fn native_generated_otlp_passes_ctsc_trace_validator_when_available() {
    let result = capture_native_rust_otlp(
        "nested_double".to_string(),
        "22222222222222222222222222222222".to_string(),
        "2222222222222201".to_string(),
        "2222222222222202".to_string(),
        r#"["2222222222222203","2222222222222204"]"#.to_string(),
        "run-native-001".to_string(),
        2_000_000_000,
        100,
        "0.5.0".to_string(),
        "rust-reference".to_string(),
        "rust".to_string(),
        2,
    );

    validate_generated_document("trace", "native-trace", &result.otlp_json);
}

#[test]
fn generated_optional_traces_match_linked_registry_when_available() {
    let registry_id = "urn:ctsc:registry:fixture.native-capture:1";
    let registry_version = "1.0.0";
    let registry = encode_schema_registry_result(
        registry_id.to_string(),
        registry_version.to_string(),
        r#"{"component":"fixture.native_capture","operations":[{"name":"echo_optional","is_async":false,"inputs":[{"name":"value","ty":"Option<string>"}],"output":"Option<string>"}],"types":[]}"#,
    )
    .expect("optional schema should encode");

    for (label, present, value, trace_id, span_prefix) in [
        ("optional-some", true, "alice", "44444444444444444444444444444444", "44444444444444"),
        (
            "optional-none",
            false,
            "ignored",
            "55555555555555555555555555555555",
            "55555555555555",
        ),
    ] {
        let result = capture_native_optional_otlp(
            label.to_string(),
            trace_id.to_string(),
            format!("{span_prefix}01"),
            format!("{span_prefix}02"),
            format!("{span_prefix}03"),
            format!("run-{label}"),
            4_000_000_000,
            100,
            "0.5.0".to_string(),
            "rust-reference".to_string(),
            "rust".to_string(),
            present,
            value.to_string(),
        );
        validate_generated_linked_documents(label, &result.otlp_json, &registry.registry_json, registry_id, registry_version);
    }
}

#[test]
fn generated_registry_passes_ctsc_registry_validator_when_available() {
    let result = encode_discovery_registry(
        "urn:ctsc:registry:fixture.stateless-add:1".to_string(),
        "1.0.0".to_string(),
        "fixture.stateless_add".to_string(),
        r#"{"operations":[{"name":"add","is_setup":false,"is_async":false,"return_type":"i32","fills":"","component":"fixture.stateless_add","params":[["a","i32"],["b","i32"]]}],"types":[]}"#.to_string(),
    );

    validate_generated_document("registry", "registry", &result.registry_json);
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
                "output":"Shape"
            }],
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

fn validate_generated_linked_documents(file_label: &str, trace_json: &str, registry_json: &str, registry_id: &str, registry_version: &str) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir.ancestors().nth(3).expect("failed to derive repository root");
    let output_dir = repo_root.join("rust").join("target");
    fs::create_dir_all(&output_dir).expect("failed to create validator output directory");
    let trace_path = output_dir.join(format!("specgate-ctsc-validator-{file_label}-trace-{}.json", std::process::id()));
    let registry_path = output_dir.join(format!("specgate-ctsc-validator-{file_label}-registry-{}.json", std::process::id()));
    fs::write(&registry_path, registry_json).expect("failed to write generated CTSC registry");

    let digest = format!("sha256:{:x}", Sha256::digest(registry_json.as_bytes()));
    let mut trace: serde_json::Value = serde_json::from_str(trace_json).expect("generated trace must be valid JSON");
    let attributes = trace["resourceSpans"][0]["resource"]["attributes"]
        .as_array_mut()
        .expect("generated trace resource attributes must be an array");
    attributes.extend([
        serde_json::json!({"key":"conformance.registry.id","value":{"stringValue":registry_id}}),
        serde_json::json!({"key":"conformance.registry.version","value":{"stringValue":registry_version}}),
        serde_json::json!({"key":"conformance.registry.digest","value":{"stringValue":digest}}),
    ]);
    fs::write(
        &trace_path,
        serde_json::to_string(&trace).expect("linked trace serialization must succeed"),
    )
    .expect("failed to write generated CTSC trace");

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
