#![cfg(test)]

// Cohesive implementation group for this subsystem.

#[cfg(test)]
mod cases {
    use super::super::execution::{failure_summary, reject_persistence};
    use super::super::facade::{capture_many, execute_tests, write_bundles};
    use super::super::filter::filter;
    use super::super::*;
    use specgate_ctsc::validation::{validate_bundle, validate_linked};

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
            .expect("repository root")
            .to_path_buf()
    }

    fn rust_binding() -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join("rust.binding.yaml")
    }

    fn csharp_binding() -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join("csharp.binding.yaml")
    }

    fn output_dir(label: impl AsRef<str>) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-capture-test-{}-{}", label.as_ref(), std::process::id()))
    }

    #[cfg(unix)]
    fn non_unicode() -> PathBuf {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        PathBuf::from(OsString::from_vec(vec![b'n', b'o', b'n', 0xff]))
    }

    #[cfg(windows)]
    fn non_unicode() -> PathBuf {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;

        PathBuf::from(OsString::from_wide(&[u16::from(b'n'), u16::from(b'o'), u16::from(b'n'), 0xd800]))
    }

    #[test]
    fn allocation_error() {
        let system = CommandEnvironment::fake();
        system.fail_next("allocation unavailable");
        let internal = scratch_dir(&system, "candidate").expect_err("injected scratch allocation must fail");
        let error = public_error(CaptureErrorKind::Execution)(internal);
        assert!(error.is_execution());
        assert_eq!(error.diagnostic(), "failed to create capture scratch directory");
    }

    #[test]
    fn stable_bundle() {
        let root = output_dir("focused");
        let _ = std::fs::remove_dir_all(&root);
        let requests = vec![
            request_at("fixture.cli.replay", root.join("batch-replay")),
            request_at("fixture.cli.replay", root.join("repeat-replay")),
            request_at("fixture.cli.setup", root.join("setup")),
            request_at("fixture.cli.multiple", root.join("multiple")),
        ];
        let discovered = discover(rust_binding().to_str().unwrap(), "").expect("focused fixture discovery");
        let executed = execute_tests(&discovered, &requests).expect("focused fixture execution");
        assert_eq!(executed.failures.len(), 1);
        assert_eq!(executed.failures[0].scenario_name, "tests::deliberately_fails_for_strict_capture");

        let reports = write_bundles(&discovered, &requests, &executed, false).expect("batched capture encoding");
        assert_eq!(reports.len(), requests.len());
        assert_eq!(
            reports[0],
            CaptureReport {
                component_id: ComponentId::new("fixture.cli.replay"),
                scenarios: 2,
                operations: 2,
                registry_path: requests[0].out.join(REGISTRY_FILE),
                trace_path: requests[0].out.join(TRACE_FILE),
                manifest_path: requests[0].out.join(MANIFEST_FILE),
            }
        );
        assert_eq!(reports[2].scenarios, 1);
        assert_eq!(reports[2].operations, 1);
        assert_eq!(reports[3].scenarios, 1);
        assert_eq!(reports[3].operations, 1);

        let batch_dir = &requests[0].out;
        let repeat_dir = &requests[1].out;
        assert_eq!(
            file_names(batch_dir),
            vec![MANIFEST_FILE.as_str(), TRACE_FILE.as_str(), REGISTRY_FILE.as_str()]
        );
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(batch_dir.join(filename)).unwrap(),
                std::fs::read(repeat_dir.join(filename)).unwrap(),
                "{filename} must be byte-identical across repeated capture"
            );
        }

        let registry = std::fs::read(batch_dir.join(REGISTRY_FILE)).unwrap();
        let trace = std::fs::read(batch_dir.join(TRACE_FILE)).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(batch_dir.join(MANIFEST_FILE)).unwrap()).unwrap();
        assert_eq!(manifest["format"], "specgate.capture-manifest");
        assert_eq!(manifest["formatVersion"], "0.1.0");
        assert_eq!(manifest["componentId"], "fixture.cli.replay");
        assert_eq!(manifest["target"], serde_json::json!({"name":"default","language":"rust"}));
        assert_eq!(
            manifest["tool"],
            serde_json::json!({"name":"specgate","version":env!("CARGO_PKG_VERSION")})
        );
        assert_eq!(manifest["registry"]["path"], REGISTRY_FILE.as_str());
        assert_eq!(manifest["registry"]["id"], "urn:ctsc:registry:fixture.cli.replay");
        assert_eq!(manifest["registry"]["version"], REGISTRY_VERSION);
        assert_eq!(manifest["registry"]["digest"], sha256_digest(&registry));
        assert_eq!(manifest["reference"]["path"], TRACE_FILE.as_str());
        assert_eq!(manifest["reference"]["digest"], sha256_digest(&trace));
        assert_eq!(manifest["scenarios"]["count"], 2);
        assert_eq!(
            manifest["scenarios"]["names"],
            serde_json::json!(["tests::adds_two_and_three", "tests::echoes_all_rust_string_escape_classes"])
        );

        // `capture` and `capture_many` differ only in request cardinality after
        // this shared execution phase. Compare both encodings without rebuilding
        // or rerunning the real fixture toolchain.
        let single = request_at("fixture.cli.replay", root.join("single-replay"));
        write_bundles(&discovered, std::slice::from_ref(&single), &executed, false).expect("single capture encoding");
        for filename in [REGISTRY_FILE, TRACE_FILE, MANIFEST_FILE] {
            assert_eq!(
                std::fs::read(batch_dir.join(filename)).unwrap(),
                std::fs::read(single.out.join(filename)).unwrap(),
                "{filename} must be byte-identical between batched and single capture"
            );
        }

        for request in &requests[0..4] {
            let linked = validate_linked(request.out.join(TRACE_FILE), request.out.join(REGISTRY_FILE), &[]);
            assert!(linked.valid, "{} linked validation failed: {:#?}", request.component, linked.issues);
            let bundle = validate_bundle(&request.out);
            assert!(bundle.valid, "{} bundle validation failed: {:#?}", request.component, bundle.issues);
        }

        let setup = read_trace(&requests[2].out);
        assert_eq!(
            inputs(&setup, "increment"),
            vec![("initial".to_string(), serde_json::json!({ "intValue": "4" }))],
            "setup construction input must be folded into the operation's public input surface"
        );

        let multiple_registry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(requests[3].out.join(REGISTRY_FILE)).unwrap()).unwrap();
        assert_eq!(
            multiple_registry["components"][0]["operations"]
                .as_array()
                .unwrap()
                .iter()
                .map(|operation| operation["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["unexercised", "used"]
        );
        let multiple_trace = read_trace(&requests[3].out);
        assert_eq!(operation_spans(&multiple_trace, "used").len(), 1);
        assert!(operation_spans(&multiple_trace, "unexercised").is_empty());

        // Component mode filters TOP-LEVEL operations only. The selected
        // component's top-level subtrees are exported verbatim — nested foreign
        // operations keep their original parents — and a foreign top-level
        // operation is dropped together with its whole subtree.
        let component_root = request_at("fixture.cli.nested_root", root.join("component-root"));
        let root_reports = write_bundles(&discovered, std::slice::from_ref(&component_root), &executed, false)
            .expect("component capture over a multi-component call chain");
        assert_eq!(root_reports[0].component_id(), "fixture.cli.nested_root");
        let root_trace = read_trace(&component_root.out);
        let root_spans = operation_spans(&root_trace, "root");
        let bridge_spans = operation_spans(&root_trace, "bridge");
        let leaf_spans = operation_spans(&root_trace, "leaf");
        assert_eq!(root_spans.len(), 2, "both passing scenarios invoke root at top level");
        assert_eq!(bridge_spans.len(), 2, "nested foreign operations are kept verbatim");
        assert_eq!(leaf_spans.len(), 2, "only the leaf reached through root survives");
        assert!(
            operation_spans(&root_trace, "sibling").is_empty(),
            "a foreign top-level operation and its subtree are dropped"
        );
        let retained_root_ids = span_ids(&root_spans);
        let retained_bridge_ids = span_ids(&bridge_spans);
        let scenario_ids = scenario_ids(&root_trace);
        assert!(
            root_spans.iter().all(|span| scenario_ids.contains(&&span["parentSpanId"])),
            "a retained top-level span keeps the scenario as its parent"
        );
        assert!(
            bridge_spans.iter().all(|span| retained_root_ids.contains(&&span["parentSpanId"])),
            "component mode never reparents a nested span"
        );
        assert!(
            leaf_spans.iter().all(|span| retained_bridge_ids.contains(&&span["parentSpanId"])),
            "component mode never reparents a nested span"
        );
        let root_registry: serde_json::Value =
            serde_json::from_slice(&std::fs::read(component_root.out.join(REGISTRY_FILE)).unwrap()).unwrap();
        assert_eq!(root_registry["registryId"], "urn:ctsc:registry:fixture.cli.nested_root");
        assert_eq!(
            root_registry["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|component| {
                    (
                        component["id"].as_str().unwrap(),
                        component["operations"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|operation| operation["name"].as_str().unwrap())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("fixture.cli.nested_bridge", vec!["bridge"]),
                ("fixture.cli.nested_leaf", vec!["leaf"]),
                ("fixture.cli.nested_root", vec!["root"]),
            ],
            "the registry must declare every component and operation present in the exported trace"
        );
        let linked = validate_linked(component_root.out.join(TRACE_FILE), component_root.out.join(REGISTRY_FILE), &[]);
        assert!(linked.valid, "component linked validation failed: {:#?}", linked.issues);
        let bundle = validate_bundle(&component_root.out);
        assert!(bundle.valid, "component bundle validation failed: {:#?}", bundle.issues);

        let component_sibling = request_at("fixture.cli.nested_sibling", root.join("component-sibling"));
        write_bundles(&discovered, std::slice::from_ref(&component_sibling), &executed, false)
            .expect("the sibling's own top-level subtree is capturable");
        let sibling_trace = read_trace(&component_sibling.out);
        assert!(operation_spans(&sibling_trace, "root").is_empty());
        assert!(operation_spans(&sibling_trace, "bridge").is_empty());
        let sibling_spans = operation_spans(&sibling_trace, "sibling");
        let sibling_leaf = operation_spans(&sibling_trace, "leaf");
        assert_eq!(sibling_spans.len(), 1);
        assert_eq!(sibling_leaf.len(), 1);
        assert_eq!(sibling_leaf[0]["parentSpanId"], sibling_spans[0]["spanId"]);

        // `leaf` is only ever reached as a nested callee, so no scenario has a
        // top-level `leaf` operation and the component contributes nothing.
        let component_leaf = request_at("fixture.cli.nested_leaf", root.join("component-leaf"));
        let leaf_error = write_bundles(&discovered, std::slice::from_ref(&component_leaf), &executed, false)
            .expect_err("a component that is never top-level contributes no scenario");
        assert!(
            leaf_error
                .diagnostic()
                .contains("no passing tests captured operations for component 'fixture.cli.nested_leaf'"),
            "{leaf_error}"
        );
        assert!(!component_leaf.out.exists());

        let unused = request_at("fixture.cli.unused", root.join("unused"));
        let no_match = write_bundles(&discovered, std::slice::from_ref(&unused), &executed, false)
            .expect_err("an unexercised component must be rejected");
        assert!(
            no_match
                .diagnostic()
                .contains("no passing tests captured operations for component 'fixture.cli.unused'")
        );
        assert!(!unused.out.exists());

        let strict = request_at("fixture.cli.replay", root.join("strict"));
        let strict_error = write_bundles(&discovered, std::slice::from_ref(&strict), &executed, true)
            .expect_err("strict capture must reject the focused failing scenario");
        assert!(strict_error.diagnostic().starts_with("1 fixture test(s) failed under capture"));
        assert!(strict_error.diagnostic().contains("tests::deliberately_fails_for_strict_capture"));
        assert!(!strict.out.exists());

        let _ = std::fs::remove_dir_all(root);
    }

    fn attribute(span: &serde_json::Value, key: impl AsRef<str>) -> Option<&serde_json::Value> {
        span["attributes"]
            .as_array()?
            .iter()
            .find(|attribute| attribute["key"] == key.as_ref())
            .map(|attribute| &attribute["value"])
    }

    fn operation_spans(trace: &serde_json::Value, operation: impl AsRef<str>) -> Vec<&serde_json::Value> {
        let operation = operation.as_ref();
        trace["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .expect("captured spans")
            .iter()
            .filter(|span| attribute(span, "conformance.operation.name").and_then(|value| value["stringValue"].as_str()) == Some(operation))
            .collect()
    }

    fn span_ids<'a>(spans: impl AsRef<[&'a serde_json::Value]>) -> Vec<&'a serde_json::Value> {
        spans.as_ref().iter().map(|span| &span["spanId"]).collect()
    }

    fn scenario_ids(trace: &serde_json::Value) -> Vec<&serde_json::Value> {
        trace["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .expect("captured spans")
            .iter()
            .filter(|span| span["name"] == "conformance.scenario")
            .map(|span| &span["spanId"])
            .collect()
    }

    fn inputs(trace: &serde_json::Value, operation: impl AsRef<str>) -> Vec<(String, serde_json::Value)> {
        let operation = operation.as_ref();
        let spans = operation_spans(trace, operation);
        assert!(!spans.is_empty(), "no captured span for operation '{operation}'");
        spans
            .iter()
            .flat_map(|span| {
                attribute(span, "conformance.operation.inputs")
                    .and_then(|value| value["kvlistValue"]["values"].as_array().cloned())
                    .unwrap_or_default()
            })
            .map(|entry| (entry["key"].as_str().unwrap_or_default().to_string(), entry["value"].clone()))
            .collect()
    }

    fn read_trace(bundle: impl AsRef<Path>) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(bundle.as_ref().join(TRACE_FILE)).expect("captured trace")).expect("valid OTLP JSON")
    }

    #[test]
    fn string_projection() {
        use specgate::ToNativeValue;

        let request = CaptureRequest::builder(CapturePaths {
            binding: "bindings/reference.yaml".into(),
            out: "capture/output".into(),
        })
        .component(" arbitrary component ! ")
        .build()
        .unwrap();
        assert_eq!(
            request.to_native_value(),
            specgate::Value::Map(std::collections::BTreeMap::from([
                (
                    "binding".to_string(),
                    specgate::Value::String("bindings/reference.yaml".to_string())
                ),
                (
                    "component".to_string(),
                    specgate::Value::String(" arbitrary component ! ".to_string())
                ),
                ("out".to_string(), specgate::Value::String("capture/output".to_string())),
                ("target".to_string(), specgate::Value::String(String::new())),
            ]))
        );

        let registry = Registry::parse(specgate::__rt::discovery().to_string()).expect("CLI metadata parses");
        let schema =
            normalize_registry(&registry, specgate_discovery::binding::Language::Rust, "specgate.cli").expect("CLI metadata normalizes");
        let operation = schema
            .operations
            .iter()
            .find(|operation| operation.name == "capture")
            .expect("capture operation metadata");
        assert_eq!(operation.inputs.len(), 1);
        assert_eq!(operation.inputs[0].name, "request");
        assert_eq!(operation.inputs[0].ty, "CaptureRequest");
        assert_eq!(operation.output.as_deref(), Some("CaptureReport"));
        assert_eq!(operation.errors.len(), 1);
        assert_eq!(operation.errors[0].ty.as_deref(), Some("CaptureError"));
        let request_type = schema
            .types
            .iter()
            .find(|candidate| candidate.name == "CaptureRequest")
            .expect("capture request metadata");
        assert_eq!(
            request_type
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.ty.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("binding", "string"),
                ("target", "string"),
                ("component", "string"),
                ("out", "string")
            ]
        );
        assert!(
            schema.types.iter().any(|candidate| candidate.name == "CaptureErrorKind"),
            "the concise Rust declaration must retain its semantic CTSC enum name"
        );
        let error_type = schema
            .types
            .iter()
            .find(|candidate| candidate.name == "CaptureError")
            .expect("capture error metadata");
        assert_eq!(
            error_type
                .fields
                .iter()
                .map(|field| (field.name.as_str(), field.ty.as_str()))
                .collect::<Vec<_>>(),
            vec![("kind", "CaptureErrorKind"), ("message", "string")]
        );
    }

    #[test]
    fn validates_defaults() {
        let binding_error = CaptureRequest::builder(CapturePaths {
            binding: PathBuf::new(),
            out: "capture/output".into(),
        })
        .build()
        .unwrap_err();
        assert!(binding_error.is_request());
        assert_eq!(binding_error.to_string(), "capture requires a non-empty binding path");

        let output_error = CaptureRequest::builder(CapturePaths {
            binding: "bindings/reference.yaml".into(),
            out: PathBuf::new(),
        })
        .build()
        .unwrap_err();
        assert!(output_error.is_request());
        assert_eq!(output_error.to_string(), "capture requires a non-empty output directory");

        let request = CaptureRequest::builder(CapturePaths {
            binding: "bindings/reference.yaml".into(),
            out: "capture/output".into(),
        })
        .build()
        .unwrap();
        assert_eq!(request.target(), "");
        assert_eq!(request.component(), "");
    }

    #[test]
    fn unicode_rejection() {
        let binding_error = CaptureRequest::builder(CapturePaths {
            binding: non_unicode(),
            out: "capture/output".into(),
        })
        .build()
        .unwrap_err();
        assert!(binding_error.is_request());
        assert_eq!(binding_error.to_string(), "capture binding path must be valid UTF-8");

        let output_error = CaptureRequest::builder(CapturePaths {
            binding: "bindings/reference.yaml".into(),
            out: non_unicode(),
        })
        .build()
        .unwrap_err();
        assert!(output_error.is_request());
        assert_eq!(output_error.to_string(), "capture output path must be valid UTF-8");
    }

    #[test]
    fn preserves_sources() {
        use std::error::Error as _;

        let context = ContextError::with_source(
            "failed to read native capture sidecar: disk unavailable",
            std::io::Error::other("disk unavailable"),
        );
        let error = public_error(CaptureErrorKind::Execution)(context);
        assert!(error.is_execution());
        assert_eq!(error.diagnostic(), "failed to read native capture sidecar: disk unavailable");
        let context_source = error.source().expect("capture context source");
        assert!(context_source.to_string().starts_with(error.diagnostic()));
        assert_eq!(
            context_source.source().map(ToString::to_string).as_deref(),
            Some("disk unavailable")
        );

        let domain = public_error(CaptureErrorKind::Selection)(ContextError::domain("select a component"));
        assert!(domain.source().is_none(), "domain diagnostics must not synthesize sources");
    }

    #[test]
    fn actionable_errors() {
        let registry = async_registry(false);
        let ambiguous = select_component(&registry, "").unwrap_err();
        assert!(ambiguous.diagnostic().contains("multiple components present") && ambiguous.diagnostic().contains("--component"));
        let unknown = select_component(&registry, "fixture.absent").unwrap_err();
        assert!(unknown.diagnostic().contains("component 'fixture.absent' not found"));

        let unsupported = discover(csharp_binding().to_str().unwrap(), "").unwrap_err();
        assert!(
            unsupported.diagnostic().contains("only Rust targets") && unsupported.diagnostic().contains("csharp"),
            "{}",
            unsupported.diagnostic()
        );
        let error = CaptureRequest::builder(CapturePaths {
            binding: rust_binding(),
            out: PathBuf::new(),
        })
        .component("fixture.cli.replay")
        .build()
        .expect_err("an empty output path must fail");
        assert!(error.is_request());
        assert!(error.to_string().contains("non-empty output directory"));
    }

    #[test]
    #[should_panic(expected = "capture_many requires at least one bundle request")]
    fn empty_batch() {
        let _ = capture_many(rust_binding().to_str().unwrap(), "", &[]);
    }

    /// One component whose only setup is async, plus a synchronous sibling
    /// component that shares the operation name.
    fn async_registry(setup_is_async: bool) -> Registry {
        let flag = if setup_is_async { "true" } else { "false" };
        let json = format!(
            r#"{{"operations":[
                {{"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.async_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":{flag},"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[["initial","i32"]],"component":"fixture.async_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.sync_setup"}},
                {{"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":false,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[["initial","i32"]],"component":"fixture.sync_setup"}}
            ],"types":[]}}"#
        );
        Registry::parse(&json).expect("synthetic registry parses")
    }

    /// Component mode exports top-level subtrees verbatim: a foreign top-level
    /// operation is dropped whole, and no retained span is ever reparented.
    #[test]
    fn component_filter() {
        let capture = synthetic_capture();
        let selected = filter(std::slice::from_ref(&capture), &ComponentId::from("fixture.root"));
        assert_eq!(
            selected[0]
                .operations
                .iter()
                .map(|operation| (operation.operation_name.as_str(), operation.parent_span_id.as_str()))
                .collect::<Vec<_>>(),
            vec![("root", "1111111111111102"), ("nested", "1111111111111103")]
        );
        assert!(
            filter(std::slice::from_ref(&capture), &ComponentId::from("fixture.nested")).is_empty(),
            "a component that is never top-level contributes nothing"
        );
        let foreign = filter(std::slice::from_ref(&capture), &ComponentId::from("fixture.other"));
        assert_eq!(
            foreign[0]
                .operations
                .iter()
                .map(|operation| operation.operation_name.as_str())
                .collect::<Vec<_>>(),
            vec!["other"]
        );
    }

    struct OperationInput<'a> {
        order: u64,
        name: &'a str,
        component: &'a str,
        span: &'a str,
        parent: &'a str,
    }

    /// One scenario with two top-level operations from different components,
    /// the first of which has a nested foreign callee.
    fn synthetic_capture() -> Capture {
        let boundary = |span_id: &str, parent: Option<&str>| {
            serde_json::json!({
                "span_id": span_id,
                "parent_span_id": parent,
                "start_time_unix_nano": 0,
                "end_time_unix_nano": 1,
                "status": "ok",
            })
        };
        let operation = |input: OperationInput<'_>| {
            serde_json::json!({
                "order": input.order,
                "component_id": input.component,
                "operation_name": input.name,
                "span_id": input.span,
                "parent_span_id": input.parent,
                "start_time_unix_nano": 0,
                "end_time_unix_nano": 1,
                "status": "ok",
                "inputs": {},
                "observations": [],
                "completion": { "kind": "empty", "order": 0, "time_unix_nano": 1 },
            })
        };
        serde_json::from_value(serde_json::json!({
            "trace_id": "11111111111111111111111111111111",
            "scenario_name": "tests::mixed",
            "run": boundary("1111111111111101", None),
            "scenario": boundary("1111111111111102", Some("1111111111111101")),
            "operations": [
                operation(OperationInput { order: 0, name: "root", component: "fixture.root", span: "1111111111111103", parent: "1111111111111102" }),
                operation(OperationInput { order: 1, name: "nested", component: "fixture.nested", span: "1111111111111104", parent: "1111111111111103" }),
                operation(OperationInput { order: 2, name: "other", component: "fixture.other", span: "1111111111111105", parent: "1111111111111102" }),
            ],
        }))
        .expect("synthetic capture deserializes")
    }

    fn request(component: impl AsRef<str>) -> BundleRequest {
        BundleRequest {
            component: ComponentId::from(component.as_ref()),
            out: PathBuf::from("unused"),
            excluded_operations: BTreeSet::new(),
        }
    }

    /// An async `#[spec_setup]` records no construction inputs, so capturing
    /// its component would encode a bundle that misstates the component's
    /// public input surface. Capture must reject it by name instead.
    #[test]
    fn async_setup() {
        let registry = async_registry(true);
        let reason = validate_setups(&registry, &[request("fixture.async_setup")]).expect_err("an async setup is not capturable");
        assert!(reason.diagnostic().contains("component 'fixture.async_setup'"), "{reason}");
        assert!(reason.diagnostic().contains("async setup 'make'"), "{reason}");
        assert!(reason.diagnostic().contains("'fixture.async_setup::advance'"), "{reason}");
        assert!(reason.diagnostic().contains("discovery-only"), "{reason}");

        assert!(
            validate_setups(&registry, &[request("fixture.sync_setup")]).is_ok(),
            "a synchronous setup on an identically named operation stays capturable"
        );
        assert!(
            validate_setups(&async_registry(false), &[request("fixture.async_setup")]).is_ok(),
            "the rejection is driven by setup metadata, not by the component name"
        );

        let batched = validate_setups(&registry, &[request("fixture.sync_setup"), request("fixture.async_setup")])
            .expect_err("every requested component is screened, not just the first");
        assert!(batched.diagnostic().contains("component 'fixture.async_setup'"), "{batched}");

        let mut excluded = request("fixture.async_setup");
        excluded.excluded_operations.insert("advance".to_string());
        assert!(
            validate_setups(&registry, &[excluded]).is_ok(),
            "the golden harness may explicitly exclude the affected operation"
        );
    }

    #[test]
    fn failure_summaries() {
        let summary = failure_summary(Some(101), b"running 1 test\ntest add ... FAILED\n", b"  \nstack backtrace: 1\n");
        assert_eq!(
            summary,
            "exit code 101; stdout: running 1 test | test add ... FAILED; stderr: stack backtrace: 1"
        );
        assert_eq!(failure_summary(None, b"", b""), "terminated without an exit code");

        let long = "x".repeat(2_000);
        let truncated = failure_summary(Some(1), long.as_bytes(), b"");
        assert!(truncated.ends_with("..."), "long output is truncated: {truncated}");
        assert!(truncated.chars().count() < 1_300);
    }

    #[test]
    fn persistence_failure() {
        reject_persistence("tests::ordinary", Some(101), b"FAILED", b"target panic").unwrap();

        let context = reject_persistence(
            "tests::persistence",
            Some(0),
            b"",
            format!("generated panic: {FAILURE_MARKER} disk full").as_bytes(),
        )
        .expect_err("the hidden marker must not become an ordinary skipped test");
        let error = public_error(CaptureErrorKind::Execution)(context);
        assert!(error.is_execution());
        assert!(error.diagnostic().contains("native capture persistence failed"));
        assert!(error.diagnostic().contains("tests::persistence"));
    }

    fn request_at(component: impl AsRef<str>, out: PathBuf) -> BundleRequest {
        BundleRequest {
            component: ComponentId::from(component.as_ref()),
            out,
            excluded_operations: BTreeSet::new(),
        }
    }

    fn file_names(path: impl AsRef<Path>) -> Vec<String> {
        let mut names = std::fs::read_dir(path.as_ref())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .map(|name| name.to_str().unwrap().to_string())
            .collect::<Vec<_>>();
        names.sort();
        names
            .into_iter()
            .map(|name| match name.as_str() {
                "manifest.json" => MANIFEST_FILE.as_str().to_owned(),
                "registry.ctsc.json" => REGISTRY_FILE.as_str().to_owned(),
                "reference.otlp.json" => TRACE_FILE.as_str().to_owned(),
                other => panic!("unexpected capture artifact: {other}"),
            })
            .collect()
    }
}
