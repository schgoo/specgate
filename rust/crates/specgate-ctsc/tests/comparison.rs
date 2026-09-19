use sha2::{Digest as _, Sha256};
use specgate_ctsc::comparison::compare;
use std::collections::BTreeMap;
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

fn scratch() -> PathBuf {
    let id = SCRATCH_ID.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("rust root")
        .join("target")
        .join(format!("ctsc-compare-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&path).expect("scratch directory");
    path
}

fn read_json(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(path).expect("fixture")).expect("fixture JSON")
}

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, serde_json::to_vec(value).expect("serialize")).expect("write fixture");
}

fn spans_mut(value: &mut serde_json::Value) -> &mut Vec<serde_json::Value> {
    value["resourceSpans"][0]["scopeSpans"][0]["spans"].as_array_mut().expect("spans")
}

fn first_span_mut<'a>(value: &'a mut serde_json::Value, name: &str) -> &'a mut serde_json::Value {
    spans_mut(value).iter_mut().find(|span| span["name"] == name).expect("named span")
}

fn attribute_mut<'a>(container: &'a mut serde_json::Value, key: &str) -> &'a mut serde_json::Value {
    container["attributes"]
        .as_array_mut()
        .expect("attributes")
        .iter_mut()
        .find(|attribute| attribute["key"] == key)
        .map(|attribute| &mut attribute["value"])
        .expect("attribute")
}

fn compare_values(
    reference: &serde_json::Value,
    candidate: &serde_json::Value,
    registry: Option<&Path>,
) -> specgate_ctsc::comparison::ComparisonReport {
    let scratch = scratch();
    let reference_path = scratch.join("reference.otlp.json");
    let candidate_path = scratch.join("candidate.otlp.json");
    write_json(&reference_path, reference);
    write_json(&candidate_path, candidate);
    let report = compare(&reference_path, &candidate_path, registry, &[]);
    std::fs::remove_dir_all(scratch).expect("remove scratch");
    report
}

#[test]
fn independent_ids_timing_and_scenario_indexes_are_ignored() {
    let reference_path = corpus().join("trace").join("valid").join("sequential.otlp.json");
    let reference = read_json(&reference_path);
    let mut candidate = reference.clone();
    let spans = spans_mut(&mut candidate);
    let replacements = spans
        .iter()
        .enumerate()
        .map(|(index, span)| {
            (
                span["spanId"].as_str().expect("span id").to_string(),
                format!("{:016x}", 0x9000_u64 + u64::try_from(index).expect("index")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for span in spans.iter_mut() {
        span["traceId"] = serde_json::json!("99999999999999999999999999999999");
        let old_id = span["spanId"].as_str().expect("span id");
        span["spanId"] = serde_json::json!(replacements[old_id]);
        if let Some(parent) = span["parentSpanId"].as_str() {
            span["parentSpanId"] = serde_json::json!(replacements[parent]);
        }
        for key in ["startTimeUnixNano", "endTimeUnixNano"] {
            let shifted = span[key].as_str().expect("time").parse::<u128>().expect("integer") + 1_000_000;
            span[key] = serde_json::json!(shifted.to_string());
        }
        if span["name"] == "conformance.scenario" {
            *attribute_mut(span, "conformance.scenario.index") = serde_json::json!({"intValue":"99"});
        }
    }
    spans.reverse();

    let report = compare_values(&reference, &candidate, None);
    assert!(report.equivalent, "{report:#?}");
    assert_eq!(report.policy, "ctsc.strict");
    assert_eq!(report.policy_version, "0.1.0");
}

#[test]
fn strict_reports_changed_result_inputs_completion_and_event_order() {
    let reference = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));
    let cases = [
        (
            "result",
            Box::new(|candidate: &mut serde_json::Value| {
                let operation = first_span_mut(candidate, "conformance.operation");
                let result = operation["events"]
                    .as_array_mut()
                    .expect("events")
                    .iter_mut()
                    .find(|event| event["name"] == "conformance.result")
                    .expect("result");
                *attribute_mut(result, "conformance.result.value") = serde_json::json!({"stringValue":"changed"});
            }) as Box<dyn Fn(&mut serde_json::Value)>,
            ".result",
        ),
        (
            "inputs",
            Box::new(|candidate: &mut serde_json::Value| {
                let operation = first_span_mut(candidate, "conformance.operation");
                let inputs = attribute_mut(operation, "conformance.operation.inputs");
                inputs["kvlistValue"]["values"][0]["value"] = serde_json::json!({"stringValue":"changed"});
            }),
            ".inputs.",
        ),
        (
            "completion",
            Box::new(|candidate: &mut serde_json::Value| {
                let operation = first_span_mut(candidate, "conformance.operation");
                let result = operation["events"]
                    .as_array_mut()
                    .expect("events")
                    .iter_mut()
                    .find(|event| event["name"] == "conformance.result")
                    .expect("result");
                *result = serde_json::json!({"name":"conformance.empty"});
            }),
            ".name",
        ),
        (
            "event-order",
            Box::new(|candidate: &mut serde_json::Value| {
                let operation = first_span_mut(candidate, "conformance.operation");
                operation["events"].as_array_mut().expect("events").swap(0, 1);
            }),
            ".observation",
        ),
    ];
    for (label, mutate, expected_path) in cases {
        let mut candidate = reference.clone();
        mutate(&mut candidate);
        let report = compare_values(&reference, &candidate, None);
        assert!(!report.equivalent, "{label}");
        assert!(
            report.mismatches.iter().any(|mismatch| mismatch.path.contains(expected_path)),
            "{label}: {report:#?}"
        );
        assert!(
            report.mismatches.iter().all(|mismatch| mismatch.path.contains("scenario[\"")),
            "{label}: {report:#?}"
        );
    }
}

#[test]
fn strict_reports_missing_and_extra_scenarios_operations_and_observations() {
    let reference = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));

    let mut missing_scenario = reference.clone();
    let scenario_id = spans_mut(&mut missing_scenario)
        .iter()
        .find(|span| span["name"] == "conformance.scenario")
        .and_then(|span| span["spanId"].as_str())
        .expect("scenario")
        .to_string();
    spans_mut(&mut missing_scenario)
        .retain(|span| span["spanId"].as_str() != Some(&scenario_id) && span["parentSpanId"].as_str() != Some(&scenario_id));
    let report = compare_values(&reference, &missing_scenario, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path.starts_with("scenario[")));

    let mut extra_scenario = reference.clone();
    let scenario = spans_mut(&mut extra_scenario)
        .iter()
        .find(|span| span["name"] == "conformance.scenario")
        .expect("scenario")
        .clone();
    let old_scenario_id = scenario["spanId"].as_str().expect("scenario id").to_string();
    let mut cloned_scenario = scenario;
    cloned_scenario["spanId"] = serde_json::json!("aaaaaaaaaaaaaa10");
    *attribute_mut(&mut cloned_scenario, "conformance.scenario.name") = serde_json::json!({"stringValue":"extra"});
    let mut cloned_operation = spans_mut(&mut extra_scenario)
        .iter()
        .find(|span| span["parentSpanId"].as_str() == Some(&old_scenario_id))
        .expect("operation")
        .clone();
    cloned_operation["spanId"] = serde_json::json!("aaaaaaaaaaaaaa11");
    cloned_operation["parentSpanId"] = serde_json::json!("aaaaaaaaaaaaaa10");
    spans_mut(&mut extra_scenario).extend([cloned_scenario, cloned_operation]);
    let report = compare_values(&reference, &extra_scenario, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path == "scenario[\"extra\"]"));

    let mut missing_operation = reference.clone();
    spans_mut(&mut missing_operation).retain(|span| span["name"] != "conformance.operation");
    let report = compare_values(&reference, &missing_operation, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path.contains(".children[0]")));

    let mut extra_operation = reference.clone();
    let mut operation = spans_mut(&mut extra_operation)
        .iter()
        .find(|span| span["name"] == "conformance.operation")
        .expect("operation")
        .clone();
    operation["spanId"] = serde_json::json!("aaaaaaaaaaaaaa01");
    operation["startTimeUnixNano"] = serde_json::json!("9999999990");
    operation["endTimeUnixNano"] = serde_json::json!("9999999999");
    spans_mut(&mut extra_operation).push(operation);
    let report = compare_values(&reference, &extra_operation, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path.contains(".children[1]")));

    let mut missing_observation = reference.clone();
    first_span_mut(&mut missing_observation, "conformance.operation")["events"]
        .as_array_mut()
        .expect("events")
        .remove(0);
    let report = compare_values(&reference, &missing_observation, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path.contains(".events[0]")));

    let mut extra_observation = reference.clone();
    let operation = first_span_mut(&mut extra_observation, "conformance.operation");
    let observation = operation["events"]
        .as_array()
        .expect("events")
        .iter()
        .find(|event| event["name"] == "conformance.observation")
        .expect("observation")
        .clone();
    operation["events"].as_array_mut().expect("events").insert(0, observation);
    let report = compare_values(&reference, &extra_observation, None);
    assert!(report.mismatches.iter().any(|mismatch| mismatch.path.contains(".events[1]")));
}

#[test]
fn strict_compares_fault_type_and_message_only() {
    let reference = read_json(&corpus().join("trace").join("valid").join("target-fault.otlp.json"));
    for key in ["conformance.fault.type", "conformance.fault.message"] {
        let mut candidate = reference.clone();
        let operation = first_span_mut(&mut candidate, "conformance.operation");
        let fault = operation["events"]
            .as_array_mut()
            .expect("events")
            .iter_mut()
            .find(|event| event["name"] == "conformance.fault")
            .expect("fault");
        *attribute_mut(fault, key) = serde_json::json!({"stringValue":"changed"});
        let report = compare_values(&reference, &candidate, None);
        assert!(
            report.mismatches.iter().any(|mismatch| mismatch.path.contains(key)),
            "{key}: {report:#?}"
        );
    }
}

#[test]
fn parallel_children_are_unordered_and_duplicate_identities_are_ambiguous() {
    let reference = read_json(&corpus().join("trace").join("valid").join("parallel.otlp.json"));
    let mut candidate = reference.clone();
    let parallel_id = spans_mut(&mut candidate)
        .iter()
        .find(|span| span["name"] == "conformance.parallel")
        .and_then(|span| span["spanId"].as_str())
        .expect("parallel")
        .to_string();
    let spans = spans_mut(&mut candidate);
    let branch_positions = spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span["parentSpanId"].as_str() == Some(&parallel_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut reordered = branch_positions.iter().map(|index| spans[*index].clone()).collect::<Vec<_>>();
    reordered.reverse();
    for (position, value) in branch_positions.into_iter().zip(reordered) {
        spans[position] = value;
    }
    let report = compare_values(&reference, &candidate, None);
    assert!(report.equivalent, "{report:#?}");

    let mut ambiguous = reference.clone();
    let mut branch = spans_mut(&mut ambiguous)
        .iter()
        .find(|span| span["parentSpanId"].as_str() == Some(&parallel_id))
        .expect("parallel branch")
        .clone();
    branch["spanId"] = serde_json::json!("bbbbbbbbbbbbbb01");
    spans_mut(&mut ambiguous).push(branch);
    let report = compare_values(&reference, &ambiguous, None);
    assert!(report.errors.iter().any(|error| error.message.contains("ambiguous")), "{report:#?}");
}

#[test]
fn parallel_identity_is_structured_when_names_contain_delimiters() {
    let mut reference = read_json(&corpus().join("trace").join("valid").join("parallel.otlp.json"));
    let parallel_id = spans_mut(&mut reference)
        .iter()
        .find(|span| span["name"] == "conformance.parallel")
        .and_then(|span| span["spanId"].as_str())
        .expect("parallel")
        .to_string();
    let branch_positions = spans_mut(&mut reference)
        .iter()
        .enumerate()
        .filter(|(_, span)| span["parentSpanId"].as_str() == Some(&parallel_id))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(branch_positions.len(), 2);
    for (position, (component, operation)) in branch_positions
        .iter()
        .copied()
        .zip([("example::orders", "price"), ("example", "orders::price")])
    {
        let branch = &mut spans_mut(&mut reference)[position];
        *attribute_mut(branch, "conformance.component.id") = serde_json::json!({"stringValue":component});
        *attribute_mut(branch, "conformance.operation.name") = serde_json::json!({"stringValue":operation});
    }

    let mut candidate = reference.clone();
    let spans = spans_mut(&mut candidate);
    let left = spans[branch_positions[0]].clone();
    spans[branch_positions[0]] = spans[branch_positions[1]].clone();
    spans[branch_positions[1]] = left;

    let report = compare_values(&reference, &candidate, None);
    assert!(report.equivalent, "{report:#?}");
}

#[test]
fn linked_sets_and_maps_compare_without_wire_order() {
    let registry = corpus().join("registry").join("valid").join("values.registry.json");
    let reference = read_json(&corpus().join("trace").join("valid").join("values.otlp.json"));
    let mut candidate = reference.clone();
    let operation = first_span_mut(&mut candidate, "conformance.operation");
    for name in ["set_value", "integer_map_value"] {
        let observation = operation["events"]
            .as_array_mut()
            .expect("events")
            .iter_mut()
            .find(|event| {
                event["attributes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|attribute| attribute["key"] == "conformance.observation.name" && attribute["value"]["stringValue"] == name)
            })
            .expect("observation");
        let value = attribute_mut(observation, "conformance.observation.value");
        value["arrayValue"]["values"].as_array_mut().expect("array value").reverse();
    }
    let report = compare_values(&reference, &candidate, Some(&registry));
    assert!(report.equivalent, "{report:#?}");
}

#[test]
fn equal_sequential_intervals_are_ambiguous_without_id_tiebreaking() {
    let mut reference = read_json(&corpus().join("trace").join("valid").join("sequential.otlp.json"));
    let operation = first_span_mut(&mut reference, "conformance.operation");
    operation["startTimeUnixNano"] = serde_json::json!("1000002000");
    operation["endTimeUnixNano"] = serde_json::json!("1000002000");
    let mut duplicate = operation.clone();
    duplicate["spanId"] = serde_json::json!("aaaaaaaaaaaaaa01");
    spans_mut(&mut reference).push(duplicate);

    let report = compare_values(&reference, &reference, None);
    assert!(report.errors.iter().any(|error| error.message.contains("ambiguous")), "{report:#?}");
}

#[test]
fn identity_mismatches_do_not_reuse_reference_types_for_candidate_payloads() {
    let scratch = scratch();
    let registry_path = scratch.join("registry.json");
    let registry_id = "urn:registry:comparison-identities";
    let registry = serde_json::json!({
        "format":"ctsc.registry",
        "formatVersion":"0.2.0",
        "registryId":registry_id,
        "version":"1.0.0",
        "components":[{
            "id":"example.compare",
            "dependencies":[],
            "operations":[
                {
                    "name":"int_op",
                    "inputs":[{"name":"value","type":{"kind":"primitive","name":"i64"}}],
                    "observations":[],
                    "outcomes":{"result":{"kind":"primitive","name":"i64"}}
                },
                {
                    "name":"string_op",
                    "inputs":[{"name":"value","type":{"kind":"primitive","name":"string"}}],
                    "observations":[],
                    "outcomes":{"result":{"kind":"primitive","name":"string"}}
                },
                {
                    "name":"observe",
                    "inputs":[],
                    "observations":[
                        {"name":"int_value","type":{"kind":"primitive","name":"i64"}},
                        {"name":"string_value","type":{"kind":"primitive","name":"string"}}
                    ],
                    "outcomes":{}
                },
                {
                    "name":"fail",
                    "inputs":[],
                    "observations":[],
                    "outcomes":{"errors":[
                        {"name":"int_error","type":{"kind":"primitive","name":"i64"}},
                        {"name":"string_error","type":{"kind":"primitive","name":"string"}}
                    ]}
                }
            ],
            "types":[]
        }]
    });
    let registry_bytes = serde_json::to_vec(&registry).expect("registry JSON");
    std::fs::write(&registry_path, &registry_bytes).expect("write registry");
    let registry_digest = format!("sha256:{:x}", Sha256::digest(&registry_bytes));

    let cases = [
        (
            linked_trace(
                registry_id,
                &registry_digest,
                "int_op",
                &serde_json::json!([{"key":"value","value":{"intValue":"7"}}]),
                &serde_json::json!([{
                    "name":"conformance.result",
                    "attributes":[{"key":"conformance.result.value","value":{"intValue":"7"}}]
                }]),
                1,
            ),
            linked_trace(
                registry_id,
                &registry_digest,
                "string_op",
                &serde_json::json!([{"key":"value","value":{"stringValue":"7"}}]),
                &serde_json::json!([{
                    "name":"conformance.result",
                    "attributes":[{"key":"conformance.result.value","value":{"stringValue":"7"}}]
                }]),
                1,
            ),
            ".operation",
        ),
        (
            linked_trace(
                registry_id,
                &registry_digest,
                "observe",
                &serde_json::json!([]),
                &serde_json::json!([{
                    "name":"conformance.observation",
                    "attributes":[
                        {"key":"conformance.observation.name","value":{"stringValue":"int_value"}},
                        {"key":"conformance.observation.value","value":{"intValue":"7"}}
                    ]
                }]),
                1,
            ),
            linked_trace(
                registry_id,
                &registry_digest,
                "observe",
                &serde_json::json!([]),
                &serde_json::json!([{
                    "name":"conformance.observation",
                    "attributes":[
                        {"key":"conformance.observation.name","value":{"stringValue":"string_value"}},
                        {"key":"conformance.observation.value","value":{"stringValue":"7"}}
                    ]
                }]),
                1,
            ),
            ".observation",
        ),
        (
            linked_trace(
                registry_id,
                &registry_digest,
                "fail",
                &serde_json::json!([]),
                &serde_json::json!([{
                    "name":"conformance.error",
                    "attributes":[
                        {"key":"conformance.error.name","value":{"stringValue":"int_error"}},
                        {"key":"conformance.error.value","value":{"intValue":"7"}}
                    ]
                }]),
                2,
            ),
            linked_trace(
                registry_id,
                &registry_digest,
                "fail",
                &serde_json::json!([]),
                &serde_json::json!([{
                    "name":"conformance.error",
                    "attributes":[
                        {"key":"conformance.error.name","value":{"stringValue":"string_error"}},
                        {"key":"conformance.error.value","value":{"stringValue":"7"}}
                    ]
                }]),
                2,
            ),
            ".error",
        ),
    ];
    for (reference, candidate, identity_path) in cases {
        let report = compare_values(&reference, &candidate, Some(&registry_path));
        assert!(report.validation_failures.is_empty(), "{report:#?}");
        assert!(
            report.mismatches.iter().any(|mismatch| mismatch.path.contains(identity_path)),
            "{report:#?}"
        );
        assert!(
            report
                .mismatches
                .iter()
                .all(|mismatch| mismatch.expected != "<invalid>" && mismatch.actual != "<invalid>"),
            "{report:#?}"
        );
    }
    std::fs::remove_dir_all(scratch).expect("remove scratch");
}

fn linked_trace(
    registry_id: &str,
    registry_digest: &str,
    operation_name: &str,
    inputs: &serde_json::Value,
    events: &serde_json::Value,
    status_code: i32,
) -> serde_json::Value {
    serde_json::json!({
        "resourceSpans":[{
            "resource":{"attributes":[
                {"key":"conformance.version","value":{"stringValue":"0.2.0"}},
                {"key":"conformance.tool.name","value":{"stringValue":"test"}},
                {"key":"conformance.tool.version","value":{"stringValue":"1.0.0"}},
                {"key":"conformance.target.name","value":{"stringValue":"test"}},
                {"key":"conformance.target.language","value":{"stringValue":"rust"}},
                {"key":"conformance.registry.id","value":{"stringValue":registry_id}},
                {"key":"conformance.registry.version","value":{"stringValue":"1.0.0"}},
                {"key":"conformance.registry.digest","value":{"stringValue":registry_digest}}
            ]},
            "scopeSpans":[{"spans":[
                {
                    "traceId":"99999999999999999999999999999999",
                    "spanId":"9999999999999901",
                    "name":"conformance.run",
                    "startTimeUnixNano":"1",
                    "endTimeUnixNano":"100",
                    "attributes":[{"key":"conformance.run.id","value":{"stringValue":"run"}}],
                    "status":{"code":1}
                },
                {
                    "traceId":"99999999999999999999999999999999",
                    "spanId":"9999999999999902",
                    "parentSpanId":"9999999999999901",
                    "name":"conformance.scenario",
                    "startTimeUnixNano":"2",
                    "endTimeUnixNano":"99",
                    "attributes":[{"key":"conformance.scenario.name","value":{"stringValue":"scenario"}}],
                    "status":{"code":status_code}
                },
                {
                    "traceId":"99999999999999999999999999999999",
                    "spanId":"9999999999999903",
                    "parentSpanId":"9999999999999902",
                    "name":"conformance.operation",
                    "startTimeUnixNano":"3",
                    "endTimeUnixNano":"98",
                    "attributes":[
                        {"key":"conformance.component.id","value":{"stringValue":"example.compare"}},
                        {"key":"conformance.operation.name","value":{"stringValue":operation_name}},
                        {"key":"conformance.operation.inputs","value":{"kvlistValue":{"values":inputs}}}
                    ],
                    "events":events,
                    "status":{"code":status_code}
                }
            ]}]
        }]
    })
}
