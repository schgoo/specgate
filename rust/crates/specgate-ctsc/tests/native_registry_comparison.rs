//! Independent native registry signature comparison.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use specgate_ctsc::comparison::compare_registry_documents;
use specgate_ctsc::validation::DocumentBytes;
use std::path::Path;

fn registry() -> Value {
    json!({
        "format": "ctsc.registry",
        "formatVersion": "0.4.0",
        "registryId": "urn:test:reference",
        "version": "1",
        "components": [{
            "id": "example.converter",
            "operations": [{
                "name": "convert",
                "inputs": [{"name": "value", "type": {"kind": "primitive", "name": "i32"}}],
                "observations": [],
                "outcomes": {}
            }],
            "types": []
        }]
    })
}

fn compare(reference: &Value, candidate: &Value) -> specgate_ctsc::comparison::RegistryComparisonReport {
    let reference = serde_json::to_vec(reference).unwrap();
    let candidate = serde_json::to_vec(candidate).unwrap();
    compare_registry_documents(
        DocumentBytes::new(Path::new("reference.registry.json"), &reference),
        &[],
        DocumentBytes::new(Path::new("candidate.registry.json"), &candidate),
        &[],
    )
}

#[test]
fn changed_primitive_input_is_a_signature_difference() {
    let reference = registry();
    let mut candidate = reference.clone();
    candidate["components"][0]["operations"][0]["inputs"][0]["type"]["name"] = json!("i64");

    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert!(!report.signatures_match);
    assert_eq!(report.differences.len(), 1);
    let difference = &report.differences[0];
    assert_eq!(
        difference.path.as_str(),
        "/components/example.converter/operations/convert/inputs/0/type/name"
    );
    assert_eq!(difference.reference, Some(json!("i32")));
    assert_eq!(difference.candidate, Some(json!("i64")));
}

#[test]
fn missing_record_fields_have_escaped_paths_and_preserve_absence() {
    let mut reference = registry();
    reference["components"][0]["types"] = json!([{
        "kind": "record", "name": "Output",
        "fields": [
            {"name": "shared", "type": {"kind": "primitive", "name": "i32"}},
            {"name": "missing/name~x", "type": {"kind": "primitive", "name": "bool"}}
        ]
    }]);
    let mut candidate = reference.clone();
    candidate["components"][0]["types"][0]["fields"].as_array_mut().unwrap().pop();

    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert_eq!(report.differences.len(), 1);
    let difference = &report.differences[0];
    assert_eq!(
        difference.path.as_str(),
        "/components/example.converter/types/Output/fields/missing~1name~0x"
    );
    assert_eq!(difference.reference, Some(json!({"kind": "primitive", "name": "bool"})));
    assert_eq!(difference.candidate, None);
}

#[test]
fn recursive_union_payloads_and_collection_properties_are_compared() {
    let mut reference = registry();
    reference["components"][0]["types"] = json!([{
        "kind": "tagged_union", "name": "Topology",
        "variants": [{"name": "Geography", "payload": {
            "kind": "tuple",
            "items": [{"kind": "collection", "ordered": true, "unique": false,
                "items": {"kind": "optional", "value": {"kind": "primitive", "name": "i32"}}}]
        }}]
    }]);
    let mut candidate = reference.clone();
    let collection = &mut candidate["components"][0]["types"][0]["variants"][0]["payload"]["items"][0];
    collection["ordered"] = json!(false);
    collection["unique"] = json!(true);
    collection["items"]["value"]["name"] = json!("i64");

    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert_eq!(report.differences.len(), 3);
    assert_eq!(
        report.differences[0].path.as_str(),
        "/components/example.converter/types/Topology/variants/Geography/payload/items/0/items/value/name"
    );
    assert_eq!(
        report.differences[1].path.as_str(),
        "/components/example.converter/types/Topology/variants/Geography/payload/items/0/ordered"
    );
    assert_eq!(report.differences[1].reference, Some(json!(true)));
    assert_eq!(report.differences[1].candidate, Some(json!(false)));
    assert!(report.differences[2].path.as_str().ends_with("/unique"));
    assert_eq!(report.differences[2].reference, Some(json!(false)));
    assert_eq!(report.differences[2].candidate, Some(json!(true)));
}

#[test]
fn artifact_identity_descriptions_and_named_member_order_are_not_signatures() {
    let mut reference = registry();
    reference["components"][0]["types"] = json!([{
        "kind": "record", "name": "Output",
        "fields": [
            {"name": "a", "type": {"kind": "primitive", "name": "i32"}},
            {"name": "b", "type": {"kind": "primitive", "name": "string"}}
        ]
    }]);
    reference["components"][0]["operations"][0]["outcomes"]["result"] = json!({"kind": "named", "name": "Output"});
    reference["components"][0]["types"].as_array_mut().unwrap().push(json!({
        "kind": "tagged_union", "name": "State",
        "variants": [{"name": "First"}, {"name": "Second"}]
    }));
    reference["components"][0]["operations"][0]["observations"] = json!([
        {"name": "one", "type": {"kind": "primitive", "name": "i32"}},
        {"name": "two", "type": {"kind": "primitive", "name": "string"}}
    ]);
    reference["components"][0]["operations"].as_array_mut().unwrap().push(json!({
        "name": "other", "inputs": [], "observations": [], "outcomes": {}
    }));
    let mut candidate = reference.clone();
    candidate["registryId"] = json!("urn:test:candidate");
    candidate["version"] = json!("2");
    candidate["description"] = json!("different artifact");
    candidate["components"][0]["description"] = json!("different documentation");
    candidate["components"][0]["operations"][0]["description"] = json!("different operation documentation");
    candidate["components"][0]["types"][0]["fields"].as_array_mut().unwrap().reverse();
    candidate["components"][0]["operations"][0]["outcomes"]["result"]["componentId"] = json!("example.converter");
    candidate["components"][0]["operations"][0]["outcomes"]["result"]["registryId"] = json!("urn:test:candidate");
    candidate["extensions"] = json!({"example.documentation": "not a signature"});
    candidate["components"][0]["operations"][0]["observations"]
        .as_array_mut()
        .unwrap()
        .reverse();
    candidate["components"][0]["types"][1]["variants"].as_array_mut().unwrap().reverse();
    candidate["components"][0]["types"].as_array_mut().unwrap().reverse();
    candidate["components"][0]["operations"].as_array_mut().unwrap().reverse();

    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert!(report.signatures_match, "{report:#?}");
    assert!(report.differences.is_empty());
}

#[test]
fn operation_input_order_remains_significant() {
    let mut reference = registry();
    reference["components"][0]["operations"][0]["inputs"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name": "second", "type": {"kind": "primitive", "name": "i32"}}));
    let mut candidate = reference.clone();
    candidate["components"][0]["operations"][0]["inputs"]
        .as_array_mut()
        .unwrap()
        .reverse();

    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert!(!report.signatures_match);
    assert_eq!(report.differences.len(), 2);
    assert_eq!(report.differences[0].reference, Some(json!("value")));
    assert_eq!(report.differences[0].candidate, Some(json!("second")));
}

#[test]
fn invalid_registries_are_not_signature_findings() {
    let mut reference = registry();
    let mut candidate = reference.clone();
    candidate["components"][0]["operations"][0]["inputs"][0]["type"] = json!({"kind": "named", "name": "Missing"});

    let report = compare(&reference, &candidate);
    assert!(!report.signatures_match);
    assert!(report.differences.is_empty());
    assert!(!report.validation_failures.is_empty());
    assert!(
        report
            .validation_failures
            .iter()
            .all(|failure| failure.location.as_str().starts_with("candidate registry:"))
    );
    reference["components"][0]["operations"][0]["inputs"][0]["type"] = json!({"kind": "named", "name": "MissingReference"});
    let both = compare(&reference, &candidate);
    assert!(!both.signatures_match);
    assert!(both.differences.is_empty());
    for side in ["reference registry:", "candidate registry:"] {
        assert!(
            both.validation_failures
                .iter()
                .any(|failure| failure.location.as_str().starts_with(side)),
            "{both:#?}"
        );
    }
}

fn imported_registry(primitive: &str, artifact: &str) -> (Vec<u8>, Vec<u8>) {
    let imported = serde_json::to_vec(&json!({
        "format": "ctsc.registry", "formatVersion": "0.4.0",
        "registryId": artifact, "version": "1",
        "components": [{
            "id": "example.shared", "operations": [],
            "types": [{"kind": "record", "name": "Payload", "fields": [
                {"name": "data", "type": {"kind": "primitive", "name": primitive}}
            ]}]
        }]
    }))
    .unwrap();
    let mut root = registry();
    root["imports"] = json!([{
        "registryId": artifact, "version": "1",
        "digest": format!("sha256:{:x}", Sha256::digest(&imported))
    }]);
    root["components"][0]["dependencies"] = json!([{
        "registryId": artifact, "componentId": "example.shared"
    }]);
    root["components"][0]["operations"][0]["inputs"][0]["type"] = json!({
        "kind": "named", "name": "Payload",
        "componentId": "example.shared", "registryId": artifact
    });
    (serde_json::to_vec(&root).unwrap(), imported)
}

#[test]
fn independent_import_closures_preserve_native_type_differences() {
    let (reference, reference_import) = imported_registry("i32", "urn:test:reference:types");
    let reference = DocumentBytes::new(Path::new("registry.json"), &reference);
    let reference_import = DocumentBytes::new(Path::new("dependency.json"), &reference_import);
    for primitive in ["i32", "i64"] {
        let (candidate, candidate_import) = imported_registry(primitive, "urn:test:candidate:types");
        let candidate = DocumentBytes::new(Path::new("registry.json"), &candidate);
        let candidate_import = DocumentBytes::new(Path::new("dependency.json"), &candidate_import);
        let report = compare_registry_documents(reference, &[reference_import], candidate, &[candidate_import]);
        assert!(report.validation_failures.is_empty(), "{report:#?}");
        if primitive == "i32" {
            assert!(report.signatures_match, "{report:#?}");
            assert!(report.differences.is_empty());
        } else {
            assert!(!report.signatures_match);
            assert_eq!(report.differences.len(), 1);
            assert_eq!(
                report.differences[0].path.as_str(),
                "/components/example.shared/types/Payload/fields/data/name"
            );
            assert_eq!(report.differences[0].reference, Some(json!("i32")));
            assert_eq!(report.differences[0].candidate, Some(json!("i64")));
        }
        let missing = compare_registry_documents(reference, &[reference_import], candidate, &[]);
        assert!(!missing.signatures_match);
        assert!(missing.differences.is_empty());
        assert!(!missing.validation_failures.is_empty());
        assert!(
            missing
                .validation_failures
                .iter()
                .all(|failure| failure.location.as_str().starts_with("candidate registry:"))
        );
    }
}

#[test]
fn observations_and_completion_declarations_remain_distinct() {
    let mut reference = registry();
    reference["components"][0]["operations"][0]["observations"] = json!([
        {"name": "state", "type": {"kind": "primitive", "name": "bool"}}
    ]);
    reference["components"][0]["operations"][0]["outcomes"] = json!({
        "result": {"kind": "primitive", "name": "bool"},
        "empty": true,
        "errors": [{"name": "Bad", "type": {"kind": "primitive", "name": "i32"}}]
    });
    let mut candidate = reference.clone();
    candidate["components"][0]["operations"][0]["observations"] = json!([]);
    candidate["components"][0]["operations"][0]["outcomes"] = json!({
        "errors": [{"name": "Bad", "type": {"kind": "primitive", "name": "i64"}}, {"name": "Extra"}]
    });
    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert_eq!(report.differences.len(), 5);
    assert!(report.differences[0].path.as_str().ends_with("/observations/state"));
    assert_eq!(report.differences[0].candidate, None);
    assert!(report.differences[1].path.as_str().ends_with("/outcomes/empty"));
    assert!(report.differences[2].path.as_str().ends_with("/outcomes/errors/Bad/type/name"));
    assert!(report.differences[3].path.as_str().ends_with("/outcomes/errors/Extra"));
    assert_eq!(report.differences[3].reference, None);
    assert_eq!(report.differences[3].candidate, Some(json!({})));
    assert!(report.differences[4].path.as_str().ends_with("/outcomes/result"));
    assert_eq!(report.differences[4].candidate, None);
}

#[test]
fn map_keys_values_and_order_are_compared_recursively() {
    let mut reference = registry();
    reference["components"][0]["operations"][0]["outcomes"]["result"] = json!({
        "kind": "map", "ordered": false,
        "keys": {"kind": "primitive", "name": "string"},
        "values": {"kind": "optional", "value": {"kind": "primitive", "name": "i32"}}
    });
    let mut candidate = reference.clone();
    let map = &mut candidate["components"][0]["operations"][0]["outcomes"]["result"];
    map["keys"]["name"] = json!("i32");
    map["values"]["value"]["name"] = json!("i64");
    map["ordered"] = json!(true);
    let report = compare(&reference, &candidate);
    assert!(report.validation_failures.is_empty(), "{report:#?}");
    assert_eq!(report.differences.len(), 3);
    assert!(report.differences[0].path.as_str().ends_with("/result/keys/name"));
    assert!(report.differences[1].path.as_str().ends_with("/result/ordered"));
    assert!(report.differences[2].path.as_str().ends_with("/result/values/value/name"));
}
