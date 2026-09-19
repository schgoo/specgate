use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use specgate_ctsc::validation::{ValidationReport, validate_registry, validate_trace};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SCRATCH_ID: AtomicU64 = AtomicU64::new(0);

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("docs")
        .join("ctsc")
        .join("corpus")
}

fn scratch(label: &str) -> PathBuf {
    let id = SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("rust root")
        .join("target")
        .join(format!("ctsc-review-{label}-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch directory");
    path
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("fixture")).expect("fixture JSON")
}

fn write_json(path: &Path, value: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec(value).expect("serialize JSON");
    std::fs::write(path, &bytes).expect("write JSON");
    bytes
}

fn validate_trace_value(label: &str, value: &Value) -> ValidationReport {
    let scratch = scratch(label);
    let path = scratch.join("trace.otlp.json");
    write_json(&path, value);
    let report = validate_trace(&path);
    std::fs::remove_dir_all(scratch).expect("remove scratch");
    report
}

fn validate_trace_text(label: &str, extension: &str, text: &str) -> ValidationReport {
    let scratch = scratch(label);
    let path = scratch.join(format!("trace.{extension}"));
    std::fs::write(&path, text).expect("write trace");
    let report = validate_trace(&path);
    std::fs::remove_dir_all(scratch).expect("remove scratch");
    report
}

fn spans_mut(value: &mut Value) -> &mut Vec<Value> {
    value["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array_mut().expect("spans")
}

fn span_mut<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    spans_mut(value).iter_mut().find(|span| span["name"] == name).expect("named span")
}

fn attributes_mut(value: &mut Value) -> &mut Vec<Value> {
    if value.get("attributes").is_none() {
        value["attributes"] = json!([]);
    }
    value["attributes"].as_array_mut().expect("attributes")
}

fn set_attribute(value: &mut Value, key: &str, attribute_value: Value) {
    if let Some(attribute) = attributes_mut(value).iter_mut().find(|attribute| attribute["key"] == key) {
        attribute["value"] = attribute_value;
    } else {
        attributes_mut(value).push(json!({"key": key, "value": attribute_value}));
    }
}

fn event_mut<'a>(span: &'a mut Value, name: &str) -> &'a mut Value {
    span["events"]
        .as_array_mut()
        .expect("events")
        .iter_mut()
        .find(|event| event["name"] == name)
        .expect("named event")
}

fn assert_invalid(report: &ValidationReport, expected: &str) {
    assert!(!report.valid, "artifact unexpectedly passed");
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.message.contains(expected) || issue.location.contains(expected)),
        "missing diagnostic containing {expected:?}: {report:#?}"
    );
}

#[test]
fn otlp_mapping_is_strict_and_accepts_official_id_and_base64_forms() {
    let base = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));

    let mut unknown = base.clone();
    span_mut(&mut unknown, "conformance.run")["unknownOtlpField"] = json!(true);
    assert_invalid(&validate_trace_value("unknown-otlp", &unknown), "unknown field");

    let mut count_overflow = base.clone();
    span_mut(&mut count_overflow, "conformance.run")["droppedAttributesCount"] = json!("4294967296");
    assert_invalid(&validate_trace_value("count-overflow", &count_overflow), "uint32");

    let mut time_overflow = base.clone();
    span_mut(&mut time_overflow, "conformance.run")["startTimeUnixNano"] = json!("18446744073709551616");
    assert_invalid(&validate_trace_value("time-overflow", &time_overflow), "uint64");

    let mut int_overflow = base.clone();
    let result = event_mut(span_mut(&mut int_overflow, "conformance.operation"), "conformance.result");
    set_attribute(result, "conformance.result.value", json!({"intValue":"9223372036854775808"}));
    assert_invalid(&validate_trace_value("int-overflow", &int_overflow), "int64");

    let mut accepted = base;
    let replacements = spans_mut(&mut accepted)
        .iter()
        .enumerate()
        .map(|(index, span)| {
            (
                span["spanId"].as_str().expect("span ID").to_string(),
                format!("ABCDEF{:010X}", index + 1),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for span in spans_mut(&mut accepted) {
        span["traceId"] = json!("ABCDEFABCDEFABCDEFABCDEFABCDEFAB");
        let old = span["spanId"].as_str().expect("span ID");
        span["spanId"] = json!(replacements[old]);
        if let Some(parent) = span["parentSpanId"].as_str() {
            span["parentSpanId"] = json!(replacements[parent]);
        }
    }
    let result = event_mut(span_mut(&mut accepted, "conformance.operation"), "conformance.result");
    set_attribute(result, "conformance.result.value", json!({"bytesValue":"-_8"}));
    let resource_spans = accepted
        .as_object_mut()
        .expect("document")
        .remove("resourceSpans")
        .expect("resource spans");
    accepted["resource_spans"] = resource_spans;
    let report = validate_trace_value("official-forms", &accepted);
    assert!(report.valid, "{report:#?}");
}

#[test]
fn optional_ctsc_attributes_have_exact_scoped_types_and_fault_diagnostics() {
    let mut fault = read_json(&corpus().join("trace").join("valid").join("target-fault.otlp.json"));
    let event = event_mut(span_mut(&mut fault, "conformance.operation"), "conformance.fault");
    for (key, value) in [
        ("conformance.fault.phase", json!({"stringValue":"invoke"})),
        ("conformance.fault.operation.name", json!({"stringValue":"price_order"})),
        ("conformance.fault.operation.component_id", json!({"stringValue":"example.pricing"})),
        ("conformance.fault.exit_code", json!({"intValue":"17"})),
        ("conformance.fault.signal", json!({"stringValue":"SIGABRT"})),
        ("conformance.fault.timeout_ms", json!({"intValue":"250"})),
    ] {
        set_attribute(event, key, value);
    }
    let report = validate_trace_value("fault-diagnostics-valid", &fault);
    assert!(report.valid, "{report:#?}");

    let parallel = read_json(&corpus().join("trace").join("valid").join("parallel.otlp.json"));
    let sequential = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));
    let invalid_cases: [(&str, Value, &str); 9] = [
        (
            "run-name",
            {
                let mut value = sequential.clone();
                set_attribute(
                    span_mut(&mut value, "conformance.run"),
                    "conformance.run.name",
                    json!({"intValue":"1"}),
                );
                value
            },
            "run.name",
        ),
        (
            "scenario-index",
            {
                let mut value = sequential.clone();
                set_attribute(
                    span_mut(&mut value, "conformance.scenario"),
                    "conformance.scenario.index",
                    json!({"stringValue":"0"}),
                );
                value
            },
            "scenario.index",
        ),
        (
            "parallel-name",
            {
                let mut value = parallel.clone();
                set_attribute(
                    span_mut(&mut value, "conformance.parallel"),
                    "conformance.parallel.name",
                    json!({"intValue":"1"}),
                );
                value
            },
            "parallel.name",
        ),
        (
            "registry-uri",
            {
                let mut value = sequential.clone();
                set_attribute(
                    &mut value["resourceSpans"][0]["resource"],
                    "conformance.registry.uri",
                    json!({"intValue":"1"}),
                );
                value
            },
            "registry.uri",
        ),
        (
            "fault-exit-code",
            {
                let mut value = fault.clone();
                set_attribute(
                    event_mut(span_mut(&mut value, "conformance.operation"), "conformance.fault"),
                    "conformance.fault.exit_code",
                    json!({"stringValue":"17"}),
                );
                value
            },
            "fault.exit_code",
        ),
        (
            "resource-key",
            {
                let mut value = sequential.clone();
                set_attribute(
                    &mut value["resourceSpans"][0]["resource"],
                    "conformance.resource.extra",
                    json!({"stringValue":"x"}),
                );
                value
            },
            "undeclared CTSC attribute",
        ),
        (
            "scope-key",
            {
                let mut value = sequential.clone();
                set_attribute(
                    &mut value["resourceSpans"][0]["scopeSpans"][0]["scope"],
                    "conformance.scope.extra",
                    json!({"stringValue":"x"}),
                );
                value
            },
            "undeclared CTSC attribute",
        ),
        (
            "event-key",
            {
                let mut value = sequential.clone();
                set_attribute(
                    event_mut(span_mut(&mut value, "conformance.operation"), "conformance.result"),
                    "conformance.result.extra",
                    json!({"stringValue":"x"}),
                );
                value
            },
            "undeclared CTSC attribute",
        ),
        (
            "operation-key",
            {
                let mut value = sequential.clone();
                set_attribute(
                    span_mut(&mut value, "conformance.operation"),
                    "conformance.operation.extra",
                    json!({"stringValue":"x"}),
                );
                value
            },
            "undeclared CTSC attribute",
        ),
    ];
    for (label, value, expected) in invalid_cases {
        assert_invalid(&validate_trace_value(label, &value), expected);
    }
    for key in [
        "conformance.fault.message",
        "conformance.fault.native_type",
        "conformance.fault.phase",
        "conformance.fault.operation.name",
        "conformance.fault.operation.component_id",
        "conformance.fault.signal",
    ] {
        let mut value = fault.clone();
        set_attribute(
            event_mut(span_mut(&mut value, "conformance.operation"), "conformance.fault"),
            key,
            json!({"intValue":"1"}),
        );
        assert_invalid(&validate_trace_value("fault-string-diagnostic", &value), key);
    }
    for key in ["conformance.fault.exit_code", "conformance.fault.timeout_ms"] {
        let mut value = fault.clone();
        set_attribute(
            event_mut(span_mut(&mut value, "conformance.operation"), "conformance.fault"),
            key,
            json!({"stringValue":"1"}),
        );
        assert_invalid(&validate_trace_value("fault-integer-diagnostic", &value), key);
    }
}

#[test]
fn operation_events_stop_at_the_terminal_event() {
    let mut value = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));
    span_mut(&mut value, "conformance.operation")["events"]
        .as_array_mut()
        .expect("events")
        .push(json!({
            "timeUnixNano":"1000006000",
            "name":"conformance.observation",
            "attributes":[
                {"key":"conformance.observation.name","value":{"stringValue":"late"}},
                {"key":"conformance.observation.value","value":{"stringValue":"late"}}
            ]
        }));
    assert_invalid(&validate_trace_value("event-after-terminal", &value), "after the terminal event");
}

#[test]
fn empty_and_whitespace_jsonl_report_no_ctsc_spans() {
    for (label, text) in [("empty-jsonl", ""), ("whitespace-jsonl", " \n\t\r\n")] {
        assert_invalid(&validate_trace_text(label, "otlp.jsonl", text), "trace contains no CTSC spans");
    }
}

#[test]
fn trace_ancestry_rejects_self_cycles_disconnected_islands_and_multiple_roots() {
    let base = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));

    let mut self_parent = base.clone();
    let operation = span_mut(&mut self_parent, "conformance.operation");
    operation["parentSpanId"] = operation["spanId"].clone();
    assert_invalid(&validate_trace_value("self-parent", &self_parent), "self-parent cycle");

    let mut cycle = base.clone();
    let operation = span_mut(&mut cycle, "conformance.operation");
    let mut second = operation.clone();
    operation["parentSpanId"] = json!("aaaaaaaaaaaaaa02");
    second["spanId"] = json!("aaaaaaaaaaaaaa02");
    second["parentSpanId"] = operation["spanId"].clone();
    spans_mut(&mut cycle).push(second);
    assert_invalid(&validate_trace_value("ancestor-cycle", &cycle), "ancestor chain contains a cycle");

    let mut disconnected = base.clone();
    span_mut(&mut disconnected, "conformance.operation")["parentSpanId"] = json!("ffffffffffffffff");
    assert_invalid(
        &validate_trace_value("disconnected-island", &disconnected),
        "must terminate at a conformance.run",
    );

    let mut multiple_ancestry = base.clone();
    let duplicate_parent = span_mut(&mut multiple_ancestry, "conformance.scenario").clone();
    spans_mut(&mut multiple_ancestry).push(duplicate_parent);
    assert_invalid(
        &validate_trace_value("multiple-ancestry", &multiple_ancestry),
        "invalid multiple ancestry",
    );

    let mut multiple_roots = base;
    let run = span_mut(&mut multiple_roots, "conformance.run").clone();
    let scenario = span_mut(&mut multiple_roots, "conformance.scenario").clone();
    let operation = span_mut(&mut multiple_roots, "conformance.operation").clone();
    let mut second_run = run;
    second_run["spanId"] = json!("aaaaaaaaaaaaaa11");
    let mut second_scenario = scenario;
    second_scenario["spanId"] = json!("aaaaaaaaaaaaaa12");
    second_scenario["parentSpanId"] = json!("aaaaaaaaaaaaaa11");
    let mut second_operation = operation;
    second_operation["spanId"] = json!("aaaaaaaaaaaaaa13");
    second_operation["parentSpanId"] = json!("aaaaaaaaaaaaaa12");
    spans_mut(&mut multiple_roots).extend([second_run, second_scenario, second_operation]);
    assert_invalid(
        &validate_trace_value("multiple-run-ancestry", &multiple_roots),
        "same conformance.run",
    );
}

#[test]
fn supervisor_fault_types_are_core_or_producer_namespaced() {
    let base = read_json(&corpus().join("trace").join("valid").join("supervisor-fault.otlp.json"));
    let mut invalid = base.clone();
    set_attribute(
        event_mut(span_mut(&mut invalid, "conformance.scenario"), "conformance.fault"),
        "conformance.fault.type",
        json!({"stringValue":"unexpected"}),
    );
    assert_invalid(
        &validate_trace_value("invalid-supervisor-fault-type", &invalid),
        "producer-qualified dotted namespace",
    );

    let mut extension = base;
    set_attribute(
        event_mut(span_mut(&mut extension, "conformance.scenario"), "conformance.fault"),
        "conformance.fault.type",
        json!({"stringValue":"example.supervisor_failure"}),
    );
    let report = validate_trace_value("namespaced-supervisor-fault-type", &extension);
    assert!(report.valid, "{report:#?}");

    let mut target = read_json(&corpus().join("trace").join("valid").join("target-fault.otlp.json"));
    set_attribute(
        event_mut(span_mut(&mut target, "conformance.operation"), "conformance.fault"),
        "conformance.fault.type",
        json!({"stringValue":"panic"}),
    );
    let report = validate_trace_value("target-native-fault-type", &target);
    assert!(report.valid, "{report:#?}");
}

#[test]
fn parallel_interval_encloses_every_direct_branch() {
    let mut value = read_json(&corpus().join("trace").join("valid").join("parallel.otlp.json"));
    let parallel_id = span_mut(&mut value, "conformance.parallel")["spanId"]
        .as_str()
        .expect("parallel ID")
        .to_string();
    let branch = spans_mut(&mut value)
        .iter_mut()
        .find(|span| span["parentSpanId"].as_str() == Some(&parallel_id))
        .expect("parallel branch");
    branch["startTimeUnixNano"] = json!("2000002999");
    assert_invalid(&validate_trace_value("parallel-enclosure", &value), "enclose every direct branch");
}

fn registry_document(registry_id: &str, component_id: &str) -> Value {
    json!({
        "format":"ctsc.registry",
        "formatVersion":"0.2.0",
        "registryId":registry_id,
        "version":"1.0.0",
        "components":[{
            "id":component_id,
            "dependencies":[],
            "operations":[],
            "types":[]
        }]
    })
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[test]
fn registry_imports_resolve_declared_candidates_by_identity_not_hint_path() {
    let scratch = scratch("registry-resolution");
    let candidate_path = scratch.join("explicit.registry.json");
    let candidate = registry_document("urn:registry:dependency", "example.dependency");
    let candidate_digest = digest(&write_json(&candidate_path, &candidate));

    let hint_path = scratch.join("wrong hint.registry.json");
    write_json(&hint_path, &registry_document("urn:registry:hint", "example.hint"));
    let hint_uri = url::Url::from_file_path(&hint_path).expect("file URL").to_string();

    let root_path = scratch.join("root.registry.json");
    let mut root = registry_document("urn:registry:root", "example.root");
    root["imports"] = json!([{
        "registryId":"urn:registry:dependency",
        "version":"1.0.0",
        "digest":candidate_digest,
        "uri":hint_uri
    }]);
    root["components"][0]["dependencies"] = json!([{"registryId":"urn:registry:dependency","componentId":"example.dependency"}]);
    write_json(&root_path, &root);
    let report = validate_registry(&root_path, std::slice::from_ref(&candidate_path));
    assert!(report.valid, "{report:#?}");

    let undeclared_path = scratch.join("undeclared.registry.json");
    write_json(&undeclared_path, &registry_document("urn:registry:undeclared", "example.root"));
    let simple_root_path = scratch.join("simple-root.registry.json");
    write_json(&simple_root_path, &registry_document("urn:registry:simple-root", "example.root"));
    let report = validate_registry(&simple_root_path, std::slice::from_ref(&undeclared_path));
    assert!(report.valid, "{report:#?}");

    let mut unused = registry_document("urn:registry:unused-root", "example.unused");
    unused["imports"] = json!([{
        "registryId":"urn:registry:missing",
        "version":"1.0.0",
        "digest":format!("sha256:{}", "0".repeat(64))
    }]);
    let unused_path = scratch.join("unused.registry.json");
    write_json(&unused_path, &unused);
    let report = validate_registry(&unused_path, &[]);
    assert!(report.valid, "{report:#?}");

    let mut used = unused;
    used["components"][0]["dependencies"] = json!([{"registryId":"urn:registry:missing","componentId":"example.missing"}]);
    let used_path = scratch.join("used.registry.json");
    write_json(&used_path, &used);
    assert_invalid(&validate_registry(&used_path, &[]), "unresolved import");

    let duplicate_a = scratch.join("duplicate-a.registry.json");
    let duplicate_b = scratch.join("duplicate-b.registry.json");
    write_json(&duplicate_a, &registry_document("urn:registry:duplicate", "example.a"));
    write_json(&duplicate_b, &registry_document("urn:registry:duplicate", "example.b"));
    assert_invalid(
        &validate_registry(&simple_root_path, &[duplicate_a, duplicate_b]),
        "ambiguous candidates",
    );

    let mut mismatch = root;
    mismatch["imports"][0]["version"] = json!("2.0.0");
    let mismatch_path = scratch.join("mismatch.registry.json");
    write_json(&mismatch_path, &mismatch);
    assert_invalid(
        &validate_registry(&mismatch_path, std::slice::from_ref(&candidate_path)),
        "version does not match",
    );
    mismatch["imports"][0]["version"] = json!("1.0.0");
    mismatch["imports"][0]["digest"] = json!(format!("sha256:{}", "f".repeat(64)));
    let digest_mismatch_path = scratch.join("digest-mismatch.registry.json");
    write_json(&digest_mismatch_path, &mismatch);
    assert_invalid(
        &validate_registry(&digest_mismatch_path, std::slice::from_ref(&candidate_path)),
        "digest does not match",
    );
    std::fs::remove_dir_all(scratch).expect("remove scratch");
}

#[test]
fn standard_percent_encoded_file_uris_resolve_cross_platform() {
    let scratch = scratch("file-uri");
    let candidate_path = scratch.join("dependency registry.json");
    let candidate = registry_document("urn:registry:file-dependency", "example.file-dependency");
    let candidate_digest = digest(&write_json(&candidate_path, &candidate));
    let uri = url::Url::from_file_path(&candidate_path).expect("file URL").to_string();
    assert!(uri.starts_with("file:///"), "{uri}");
    assert!(uri.contains("%20"), "{uri}");

    let root_path = scratch.join("root.registry.json");
    let mut root = registry_document("urn:registry:file-root", "example.file-root");
    root["imports"] = json!([{
        "registryId":"urn:registry:file-dependency",
        "version":"1.0.0",
        "digest":candidate_digest,
        "uri":uri
    }]);
    root["components"][0]["dependencies"] = json!([{"registryId":"urn:registry:file-dependency","componentId":"example.file-dependency"}]);
    write_json(&root_path, &root);
    let report = validate_registry(&root_path, &[]);
    std::fs::remove_dir_all(scratch).expect("remove scratch");
    assert!(report.valid, "{report:#?}");
}

#[test]
fn registry_auto_resolution_rejects_remote_authorities_and_unc_paths() {
    let scratch = scratch("network-file-uri");
    for (label, uri) in [
        ("remote-authority", "file://registry-host/share/dependency.registry.json"),
        ("unc-slashes", "file:////registry-host/share/dependency.registry.json"),
        ("unc-backslashes", "file:%5C%5Cregistry-host%5Cshare%5Cdependency.registry.json"),
    ] {
        let root_path = scratch.join(format!("{label}.registry.json"));
        let mut root = registry_document("urn:registry:network-root", "example.network-root");
        root["imports"] = json!([{
            "registryId":"urn:registry:network-dependency",
            "version":"1.0.0",
            "digest":format!("sha256:{}", "0".repeat(64)),
            "uri":uri
        }]);
        root["components"][0]["dependencies"] =
            json!([{"registryId":"urn:registry:network-dependency","componentId":"example.network-dependency"}]);
        write_json(&root_path, &root);
        let report = validate_registry(&root_path, &[]);
        assert_invalid(&report, "network");
        assert_invalid(&report, "--import");
    }
    std::fs::remove_dir_all(scratch).expect("remove scratch");
}

#[test]
fn registry_json_schema_shape_and_extension_rules_are_enforced() {
    let base = registry_document("urn:registry:schema", "example.schema");
    let valid_extensions = {
        let mut value = base.clone();
        value["extensions"] = json!({"vendor.feature":null,"vendor.config":{"enabled":true}});
        value
    };
    let scratch = scratch("registry-schema");
    let valid_path = scratch.join("valid.registry.json");
    write_json(&valid_path, &valid_extensions);
    let report = validate_registry(&valid_path, &[]);
    assert!(report.valid, "{report:#?}");

    let invalid_cases: [(&str, Value); 6] = [
        ("unknown-named-type-property", {
            let mut value = base.clone();
            value["components"][0]["types"] = json!([{
                "name":"Record",
                "kind":"record",
                "fields":[],
                "unknown":true
            }]);
            value
        }),
        ("null-description", {
            let mut value = base.clone();
            value["description"] = Value::Null;
            value
        }),
        ("empty-dependency-id", {
            let mut value = base.clone();
            value["components"][0]["dependencies"] = json!([{"componentId":""}]);
            value
        }),
        ("empty-named-component-id", {
            let mut value = base.clone();
            value["components"][0]["operations"] = json!([{
                "name":"run",
                "inputs":[{"name":"value","type":{"kind":"named","name":"Thing","componentId":""}}],
                "observations":[],
                "outcomes":{}
            }]);
            value
        }),
        ("reserved-extension", {
            let mut value = base.clone();
            value["extensions"] = json!({"conformance.private":true});
            value
        }),
        ("invalid-extension-shape", {
            let mut value = base;
            value["extensions"] = json!({"Vendor":true});
            value
        }),
    ];
    for (label, value) in invalid_cases {
        let path = scratch.join(format!("{label}.registry.json"));
        write_json(&path, &value);
        assert!(!validate_registry(&path, &[]).valid, "{label} unexpectedly passed");
    }
    std::fs::remove_dir_all(scratch).expect("remove scratch");
}
