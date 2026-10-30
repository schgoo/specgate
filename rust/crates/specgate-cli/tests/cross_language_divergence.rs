//! Fault-injection witnesses over native C# recordings replayed on Rust.

use serde_json::{Value, json};
use specgate_ctsc::{compare_documents, comparison::ComparisonReport, validation::DocumentBytes};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .to_path_buf()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("native capture artifact")).expect("native capture JSON")
}

fn attribute<'a>(container: &'a Value, key: &str) -> &'a Value {
    &container["attributes"]
        .as_array()
        .expect("attributes")
        .iter()
        .find(|attribute| attribute["key"] == key)
        .expect("named attribute")["value"]
}

fn attribute_mut<'a>(container: &'a mut Value, key: &str) -> &'a mut Value {
    &mut container["attributes"]
        .as_array_mut()
        .expect("attributes")
        .iter_mut()
        .find(|attribute| attribute["key"] == key)
        .expect("named attribute")["value"]
}

fn operation<'a>(trace: &'a mut Value, name: &str) -> &'a mut Value {
    trace["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array_mut()
        .expect("native spans")
        .iter_mut()
        .find(|span| span["name"] == "conformance.operation" && attribute(span, "conformance.operation.name")["stringValue"] == name)
        .expect("captured operation")
}

fn event<'a>(operation: &'a mut Value, name: &str) -> &'a mut Value {
    operation["events"]
        .as_array_mut()
        .expect("native events")
        .iter_mut()
        .find(|event| event["name"] == name)
        .expect("captured event")
}

fn compare(reference: &Value, candidate: &Value) -> ComparisonReport {
    let reference = serde_json::to_vec(reference).expect("reference JSON");
    let candidate = serde_json::to_vec(candidate).expect("candidate JSON");
    compare_documents(
        DocumentBytes::new(Path::new("csharp-reference.otlp.json"), &reference),
        DocumentBytes::new(Path::new("rust-candidate.otlp.json"), &candidate),
        None,
        [],
    )
}

fn assert_valid(report: &ComparisonReport) {
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert!(report.errors.is_empty(), "{report:#?}");
    assert_eq!(report.policy, "ctsc.strict");
    assert_eq!(report.policy_version, "0.2.0");
}

fn assert_mismatch(reference: &Value, candidate: &Value, path: &str) {
    let report = compare(reference, candidate);
    assert_valid(&report);
    assert!(!report.equivalent, "{report:#?}");
    assert!(
        report.mismatches.iter().any(|mismatch| mismatch.path.contains(path)),
        "expected {path}: {report:#?}"
    );
}

fn replay(component: &str, scratch: &Path) -> (Value, Value) {
    let root = repo_root();
    let bundle = root.join("test").join("behavior").join("cases").join(component).join("expected");
    let candidate = scratch.join(format!("{component}.otlp.json"));
    let output = Command::new(env!("CARGO_BIN_EXE_specgate"))
        .arg("replay")
        .arg(&bundle)
        .arg("--binding")
        .arg(root.join("test").join("behavior").join("specgate.yaml"))
        .args(["--impl", "rust", "--out"])
        .arg(&candidate)
        .output()
        .expect("Rust candidate replay");
    assert!(
        output.status.success(),
        "{component}: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let reference = read_json(&bundle.join("reference.otlp.json"));
    let candidate = read_json(&candidate);
    assert_eq!(
        attribute(&reference["resourceSpans"][0]["resource"], "conformance.target.name")["stringValue"],
        "expected",
        "native provenance follows the flat bundle directory basename"
    );
    for (trace, language) in [(&reference, "csharp"), (&candidate, "rust")] {
        assert_eq!(
            attribute(&trace["resourceSpans"][0]["resource"], "conformance.target.language")["stringValue"],
            language
        );
    }
    let report = compare(&reference, &candidate);
    assert_valid(&report);
    assert!(report.equivalent, "{component}: {report:#?}");
    (reference, candidate)
}

#[test]
fn strict_detects_cross_language_inputs_results_errors_nested_calls_and_event_order() {
    let scratch = tempfile::tempdir().expect("candidate traces");
    let (reference, candidate) = replay("fixture.stateless_add", scratch.path());
    let mut changed = candidate.clone();
    attribute_mut(operation(&mut changed, "add"), "conformance.operation.inputs")["kvlistValue"]["values"][0]["value"] =
        json!({"intValue": "999"});
    assert_mismatch(&reference, &changed, ".inputs.");

    let mut changed = candidate;
    *attribute_mut(
        event(operation(&mut changed, "add"), "conformance.result"),
        "conformance.result.value",
    ) = json!({"intValue": "999"});
    assert_mismatch(&reference, &changed, ".result");

    let (reference, mut candidate) = replay("fixture.result_err", scratch.path());
    *attribute_mut(
        event(operation(&mut candidate, "try_divide"), "conformance.error"),
        "conformance.error.value",
    ) = json!({"stringValue": "changed error"});
    assert_mismatch(&reference, &candidate, "::try_divide].events[0].value");

    let (mut reference, mut candidate) = replay("fixture.checkpoint_inline", scratch.path());
    let mut changed = candidate.clone();
    let parent = operation(&mut changed, "process")["spanId"].clone();
    let nested = changed["resourceSpans"][0]["scopeSpans"][0]["spans"]
        .as_array_mut()
        .expect("native spans")
        .iter_mut()
        .find(|span| span["parentSpanId"] == parent)
        .expect("nested operation under process");
    assert_eq!(attribute(nested, "conformance.operation.name")["stringValue"], "after_upper");
    *attribute_mut(nested, "conformance.operation.name") = json!({"stringValue": "changed_nested_operation"});
    assert_mismatch(&reference, &changed, "::process].children[0].operation");

    // The retained native corpus has one completion event per operation. Inject
    // identical Trace Core observations into both independent producer traces
    // before changing only the candidate event order; no golden is modified.
    for trace in [&mut reference, &mut candidate] {
        let operation = operation(trace, "process");
        let events = operation["events"].as_array_mut().expect("events");
        let time = events[0]["timeUnixNano"].clone();
        for (index, name) in ["first", "second"].into_iter().enumerate() {
            events.insert(
                index,
                json!({
                    "name": "conformance.observation",
                    "timeUnixNano": time,
                    "attributes": [
                        {"key": "conformance.observation.name", "value": {"stringValue": name}},
                        {"key": "conformance.observation.value", "value": {"stringValue": name}}
                    ]
                }),
            );
        }
    }
    let report = compare(&reference, &candidate);
    assert_valid(&report);
    assert!(report.equivalent, "{report:#?}");
    operation(&mut candidate, "process")["events"]
        .as_array_mut()
        .expect("events")
        .swap(0, 1);
    assert_mismatch(&reference, &candidate, ".observation");
}
