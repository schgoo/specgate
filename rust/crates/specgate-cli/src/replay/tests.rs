#![cfg(test)]

// Cohesive implementation group for this subsystem.

#[cfg(test)]
mod cases {
    use super::super::error::FailureStage;
    use super::super::runner::{cargo_manifest, source};
    use super::super::*;
    use crate::capture_impl::{CapturePaths, CaptureRequest, capture};
    use specgate::ComponentId;
    use specgate_ctsc::{
        replay::model as replay_model,
        validation::{validate_linked, validate_trace},
    };
    use specgate_discovery::schema::Input;

    fn repo_root() -> PathBuf {
        std::env::current_dir()
            .unwrap()
            .ancestors()
            .find(|path| path.join("rust").join("Cargo.toml").is_file())
            .expect("repository root")
            .to_path_buf()
    }

    fn fixture_binding() -> PathBuf {
        repo_root()
            .join("rust")
            .join("crates")
            .join("specgate-cli")
            .join("tests")
            .join("fixtures")
            .join("specgate.yaml")
    }

    fn output_dir(label: impl AsRef<str>) -> PathBuf {
        repo_root()
            .join("rust")
            .join("target")
            .join(format!("specgate-replay-test-{}-{}", label.as_ref(), std::process::id()))
    }

    fn cargo_path(path: impl AsRef<Path>) -> String {
        let display = path.as_ref().display().to_string();
        display.strip_prefix(r"\\?\").unwrap_or(&display).replace('\\', "/")
    }

    fn configured_candidate() -> (tempfile::TempDir, PathBuf) {
        let cache = tempfile::Builder::new().prefix("candidate-cargo-config-").tempdir().unwrap();
        let candidate = cache.path().join("candidate");
        let proof = cache.path().join("specgate-config-proof");
        std::fs::create_dir_all(candidate.join(".cargo")).unwrap();
        std::fs::create_dir_all(candidate.join("src")).unwrap();
        std::fs::create_dir_all(proof.join("src")).unwrap();
        std::fs::write(
            proof.join("Cargo.toml"),
            "[package]\nname=\"specgate-config-proof\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[workspace]\n",
        )
        .unwrap();
        std::fs::write(proof.join("src").join("lib.rs"), "pub fn add(a: i32, b: i32) -> i32 { a + b }\n").unwrap();
        std::fs::write(
            candidate.join("Cargo.toml"),
            format!(
                "[package]\nname=\"configured-candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n\
                 [dependencies]\nspecgate={{path=\"{}\"}}\nspecgate-config-proof=\"0.1.0\"\n[workspace]\n",
                cargo_path(repo_root().join("rust").join("crates").join("specgate"))
            ),
        )
        .unwrap();
        // Keep the isolated graph exact while allowing clean hosts to fetch target-specific crates absent from their cache.
        let mut lockfile = include_str!("../../../../Cargo.lock").to_string();
        lockfile.push_str(
            "\n[[package]]\nname = \"configured-candidate\"\nversion = \"0.1.0\"\n\
             dependencies = [\n \"specgate\",\n \"specgate-config-proof\",\n]\n\
             \n[[package]]\nname = \"specgate-config-proof\"\nversion = \"0.1.0\"\n",
        );
        std::fs::write(candidate.join("Cargo.lock"), lockfile).unwrap();
        std::fs::write(
            candidate.join("src").join("lib.rs"),
            "#![allow(unexpected_cfgs, reason = \"the fixture intentionally verifies a rustflags-provided custom cfg\")]\n\
             #[cfg(not(specgate_candidate_config))]\n\
             compile_error!(\"candidate .cargo/config.toml was not loaded\");\n\
             use specgate::{spec_component, spec_operation};\n\
             spec_component!(\"fixture.configured\");\n\
             #[spec_operation(\"add\")]\n\
             pub fn add(a: i32, b: i32) -> i32 { specgate_config_proof::add(a, b) }\n\
             #[test]\n\
             fn adds_two_and_three() { assert_eq!(add(2, 3), 5); }\n",
        )
        .unwrap();
        std::fs::write(
            candidate.join(".cargo").join("config.toml"),
            format!(
                "[build]\nrustflags=[\"--cfg\", \"specgate_candidate_config\"]\n\
                 [patch.crates-io]\nspecgate-config-proof={{path=\"{}\"}}\n",
                cargo_path(&proof)
            ),
        )
        .unwrap();
        let binding = cache.path().join("binding.yaml");
        std::fs::write(
            &binding,
            format!(
                "components:\n  fixture.configured:\n    implementations:\n      rust:\n        language: rust\n        sources: {}\n        tests:\n          projects:\n            - {}\n",
                cargo_path(&candidate),
                cargo_path(candidate.join("Cargo.toml"))
            ),
        )
        .unwrap();
        (cache, binding)
    }

    #[test]
    fn replay_contract() {
        let capture_dir = output_dir("capture");
        let first_output = output_dir("first").join("candidate.otlp.json");
        let second_output = output_dir("second").join("candidate.otlp.json");
        let _ = std::fs::remove_dir_all(&capture_dir);
        let _ = std::fs::remove_dir_all(first_output.parent().unwrap());
        let _ = std::fs::remove_dir_all(second_output.parent().unwrap());
        std::fs::create_dir_all(first_output.parent().unwrap()).unwrap();
        std::fs::create_dir_all(second_output.parent().unwrap()).unwrap();

        let captured = capture(
            CaptureRequest::builder(CapturePaths {
                binding: fixture_binding(),
            })
            .component(ComponentId::from("fixture.cli.replay"))
            .out(&capture_dir)
            .implementation("rust")
            .build()
            .unwrap(),
        );
        assert!(captured.is_ok(), "capture failed: {captured:?}");
        let component = specgate_discovery::identity::ComponentId::from("fixture.cli.replay");
        let candidates = Candidates::discover(fixture_binding(), Some(&"rust".into()), std::slice::from_ref(&component))
            .expect("focused candidate discovery");
        let candidates = Candidates::discover_with(
            &DiscoveryInput {
                binding: Path::new("unused"),
                implementation: None,
                components: &[],
            },
            &DiscoveryContext {
                discovery: &Discovery::real(),
                system: &CommandEnvironment::fake(),
                discovered: Some(candidates),
            },
        )
        .expect("injected discovery");
        let plan = candidates.plan(&capture_dir).expect("focused replay plan");
        let reports = candidates
            .execute(&[(plan.clone(), first_output.clone()), (plan, second_output.clone())])
            .expect("focused replay execution");
        assert_eq!(reports.len(), 2);
        assert_eq!(
            reports[0],
            Report {
                component_id: ComponentId::from("fixture.cli.replay"),
                scenarios: 2,
                operations: 2,
                plans: 2,
                output_path: first_output.clone(),
            }
        );
        let first_bytes = std::fs::read(&first_output).unwrap();
        assert_eq!(first_bytes, std::fs::read(&second_output).unwrap());

        let reference: serde_json::Value = serde_json::from_slice(&std::fs::read(capture_dir.join(reference_file())).unwrap()).unwrap();
        let candidate: serde_json::Value = serde_json::from_slice(&first_bytes).unwrap();
        let reference_trace = &reference["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"];
        let candidate_trace = &candidate["resourceSpans"][0]["scopeSpans"][0]["spans"][0]["traceId"];
        assert_ne!(reference_trace, candidate_trace);
        let candidate_text = String::from_utf8(first_bytes).unwrap();
        assert!(candidate_text.contains("\"conformance.target.name\""));
        assert!(candidate_text.contains("candidate:specgate-cli-fixtures:candidate"));
        assert!(candidate_text.contains("\"conformance.operation.name\""));
        assert!(candidate_text.contains("\"add\""));
        assert!(candidate_text.contains("\"a\""));
        assert!(candidate_text.contains("\"intValue\":\"2\""));
        assert!(candidate_text.contains("\"b\""));
        assert!(candidate_text.contains("\"intValue\":\"3\""));
        assert!(candidate_text.contains("\"conformance.result\""));
        assert!(candidate_text.contains("\"intValue\":\"5\""));

        let echo = candidate["resourceSpans"][0]["scopeSpans"][0]["spans"]
            .as_array()
            .unwrap()
            .iter()
            .find(|span| {
                span["attributes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|attribute| attribute["key"] == "conformance.operation.name" && attribute["value"]["stringValue"] == "echo")
            })
            .expect("echo operation span");
        let inputs = echo["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|attribute| attribute["key"] == "conformance.operation.inputs")
            .unwrap();
        assert_eq!(
            inputs["value"]["kvlistValue"]["values"][0]["value"]["stringValue"],
            "nul:\0 backspace:\u{8} formfeed:\u{c} quote:\" slash:\\ cr:\r lf:\n tab:\t unicode:雪🙂"
        );

        let trace_validation = validate_trace(&first_output);
        assert!(
            trace_validation.valid,
            "native trace validation failed: {:#?}",
            trace_validation.issues
        );
        let linked_validation = validate_linked(&first_output, capture_dir.join(registry_file()), &[]);
        assert!(
            linked_validation.valid,
            "native linked validation failed: {:#?}",
            linked_validation.issues
        );

        let unsupported_output = output_dir("unsupported-language").with_extension("otlp.json");
        assert!(matches!(
            replay(Request::builder(Paths::new(&capture_dir, fixture_binding())).out(&unsupported_output).implementation("csharp").build().unwrap()),
            Err(error) if error.to_string().contains("only Rust candidates") && error.to_string().contains("csharp")
        ));
        assert!(!unsupported_output.exists());

        let ambiguous_output = output_dir("ambiguous-implementation").with_extension("otlp.json");
        let ambiguous = replay(
            Request::builder(Paths::new(&capture_dir, fixture_binding()))
                .out(&ambiguous_output)
                .build()
                .unwrap(),
        )
        .expect_err("a component with several implementations requires --impl");
        assert!(ambiguous.to_string().contains("csharp, rust"), "{ambiguous}");
        assert!(!ambiguous_output.exists());

        let _ = std::fs::remove_dir_all(capture_dir);
        let _ = std::fs::remove_file(first_output);
        let _ = std::fs::remove_file(second_output);
    }

    #[test]
    fn typed_async_setup_replay() {
        typed_setup_replay("typed-async.yaml", "rust");
    }

    #[test]
    fn arty_runtime_replay() {
        setup_replay("arty-replay.yaml", "configured", "fixture.arty_replay", 1, 1);
    }

    #[test]
    fn arty_custom_executor_replay() {
        setup_replay_to("arty-replay.yaml", "configured", "configured", "fixture.arty_replay", 1, 1);
    }

    #[test]
    fn typed_integration_setup_replay() {
        typed_setup_replay("typed-integration.yaml", "rust");
    }

    #[test]
    fn typed_csharp_to_rust_setup_replay() {
        typed_setup_replay("typed-integration.yaml", "csharp");
    }

    #[test]
    fn private_lexical_setup_replay() {
        let bundle = setup_replay("private-replay.yaml", "rust", "fixture.private_replay", 12, 4);
        assert_eq!(bundle.observed_operations.len(), 13);
        assert!(
            bundle
                .observed_operations
                .iter()
                .any(|operation| operation.operation_name == "inner")
        );
        assert!(
            !bundle
                .scenarios
                .iter()
                .flat_map(|scenario| &scenario.operations)
                .any(|operation| operation.operation_name == "inner")
        );
    }

    #[test]
    fn test_only_operation_replay() {
        setup_replay("private-replay.yaml", "rust", "fixture.test_only", 1, 1);
    }

    #[test]
    fn integration_local_operation_replay() {
        setup_replay("typed-integration.yaml", "rust", "fixture.integration_local", 1, 1);
    }

    fn typed_setup_replay(binding_name: &str, reference: &str) {
        setup_replay(binding_name, reference, "fixture.typed_async", 3, 1);
    }

    fn setup_replay(binding_name: &str, reference: &str, component: &str, operations: u32, scenarios: u32) -> ReplayBundle {
        setup_replay_to(binding_name, reference, "rust", component, operations, scenarios)
    }

    fn setup_replay_to(
        binding_name: &str,
        reference: &str,
        candidate: &str,
        component: &str,
        operations: u32,
        scenarios: u32,
    ) -> ReplayBundle {
        let cache = tempfile::tempdir().unwrap();
        let binding = fixture_binding().with_file_name(binding_name);
        let capture_dir = cache.path().join("capture");
        let output = cache.path().join("candidate.otlp.json");
        capture(
            CaptureRequest::builder(CapturePaths { binding: binding.clone() })
                .component(ComponentId::from(component))
                .implementation(reference)
                .out(&capture_dir)
                .build()
                .unwrap(),
        )
        .expect("capture a generic async instance operation with typed results");
        let bundle = specgate_ctsc::replay::decode(
            std::fs::read(capture_dir.join(manifest_file())).unwrap(),
            std::fs::read(capture_dir.join(registry_file())).unwrap(),
            std::fs::read(capture_dir.join(reference_file())).unwrap(),
        )
        .unwrap();
        let report = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(&output)
                .implementation(candidate)
                .build()
                .unwrap(),
        )
        .expect("replay selected setups and preserve the shared receiver");
        assert_eq!(report.operations, operations);
        assert_eq!(report.scenarios, scenarios);
        let validation = validate_linked(&output, capture_dir.join(registry_file()), &[]);
        assert!(validation.valid, "{:#?}", validation.issues);
        let compared = specgate_ctsc::comparison::compare_with(
            capture_dir.join(reference_file()),
            &output,
            Some(capture_dir.join(registry_file())),
            [],
            &specgate_ctsc::comparison::SystemReader::system(),
        );
        assert!(compared.equivalent, "{compared:#?}");
        bundle
    }

    #[test]
    fn replay_selects_candidate_executor_identity_and_rejects_ambiguity() {
        use specgate_discovery::{
            binding::{LanguageOptions, Runtime},
            runner::PackageSource,
        };
        let mut raw = raw_operation("add");
        raw.is_async = true;
        let mut normalized = normalized_operation("add");
        normalized.is_async = true;
        let mut candidate = candidate_metadata(raw, normalized);
        assert_error(&candidate, "no matching replay executor");
        for (name, version) in [("arty", "0.5.0"), ("smol", "2.0.2"), ("tokio", "1.48.0")] {
            Arc::make_mut(&mut candidate.inner).executors = BTreeMap::from([(
                "engine".to_string(),
                PackageSource::registry(name, version, "registry+https://github.com/rust-lang/crates.io-index").into(),
            )]);
            let plan = build_plan(&reference_bundle(), &candidate).unwrap();
            let manifest = cargo_manifest(&plan).unwrap().manifest;
            assert!(manifest.contains(&format!("={version}")), "{manifest}");
            assert!(manifest.contains("engine"), "{manifest}");
            let code = source(&plan);
            assert!(
                code.contains(if name == "arty" {
                    "engine::runtime::Runtime::new"
                } else if name == "smol" {
                    "engine::block_on"
                } else {
                    "engine::runtime::Builder"
                }),
                "{code}"
            );
        }
        Arc::make_mut(&mut candidate.inner)
            .executors
            .insert("arty".to_string(), PackageSource::local("arty", "0.5.0", "arty").into());
        assert_error(&candidate, "multiple matching replay executors");
        Arc::make_mut(&mut candidate.inner).executor_options = LanguageOptions::Rust {
            runtime: Runtime::Arty,
            executor: None,
        };
        let plan = build_plan(&reference_bundle(), &candidate).unwrap();
        assert!(matches!(plan.target.executor, model::Executor::Builtin { ref alias, .. } if alias == "arty"));
        Arc::make_mut(&mut candidate.inner).executors.get_mut("arty").unwrap().source =
            Err("unsupported arty Cargo source 'git+https://example.test/arty'".to_string());
        assert_error(&candidate, "configure rust.executor");
        Arc::make_mut(&mut candidate.inner).executor_options = LanguageOptions::Rust {
            runtime: Runtime::Auto,
            executor: Some("crate::run_replay".to_string()),
        };
        let plan = build_plan(&reference_bundle(), &candidate).unwrap();
        assert!(source(&plan).contains("candidate::run_replay(move || async move"));
        assert!(!cargo_manifest(&plan).unwrap().manifest.contains("[dependencies.engine]"));
    }

    #[test]
    fn candidate_config() {
        let (cache, binding) = configured_candidate();
        let capture_dir = cache.path().join("capture");
        let replay_output = cache.path().join("candidate.otlp.json");
        let captured = capture(
            CaptureRequest::builder(CapturePaths { binding: binding.clone() })
                .component(ComponentId::from("fixture.configured"))
                .out(&capture_dir)
                .build()
                .unwrap(),
        );
        assert!(captured.is_ok(), "capture failed: {captured:?}");
        let replayed = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(&replay_output)
                .build()
                .unwrap(),
        );
        assert!(
            matches!(
                &replayed,
                Ok(Report {
                    component_id,
                    scenarios: 1,
                    operations: 1,
                    plans: 1,
                    ..
                }) if component_id.as_str() == "fixture.configured"
            ),
            "replay failed: {replayed:?}"
        );
        let output = std::fs::read_to_string(&replay_output).unwrap();
        assert!(
            output.contains("candidate:configured-candidate:candidate"),
            "target label is the output file stem"
        );

        let refused = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(&replay_output)
                .build()
                .unwrap(),
        )
        .expect_err("an existing output file is refused");
        assert!(refused.to_string().contains("already exists; pass --force"), "{refused}");
        let forced = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(&replay_output)
                .force(true)
                .build()
                .unwrap(),
        );
        assert!(forced.is_ok(), "--force overwrites: {forced:?}");

        let empty_dir = output_dir("empty-out");
        let _ = std::fs::remove_dir_all(&empty_dir);
        std::fs::create_dir_all(&empty_dir).unwrap();
        let refused_dir = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(&empty_dir)
                .build()
                .unwrap(),
        )
        .expect_err("an existing empty directory is refused");
        assert!(refused_dir.to_string().contains("already exists; pass --force"), "{refused_dir}");
        std::fs::remove_dir_all(&empty_dir).unwrap();

        let unlabeled = replay(
            Request::builder(Paths::new(&capture_dir, &binding))
                .out(capture_dir.join("x").join(".otlp.json"))
                .build()
                .unwrap(),
        )
        .expect_err("an empty label is refused");
        assert!(unlabeled.to_string().contains("has no label"), "{unlabeled}");

        let defaulted = replay(Request::builder(Paths::new(&capture_dir, &binding)).build().unwrap()).expect("default output replay");
        assert_eq!(defaulted.output_path, capture_dir.join("candidate-rust-capture.otlp.json"));
        let output = std::fs::read_to_string(&defaulted.output_path).unwrap();
        assert!(output.contains("candidate:configured-candidate:candidate-rust-capture"));
    }

    #[test]
    fn distinct_roots() {
        let system = CommandEnvironment::fake();
        let first = runner::scratch(&system).unwrap();
        let second = runner::scratch(&system).unwrap();
        assert_ne!(first.path(), second.path());
    }

    #[test]
    fn scratch_ownership() {
        let system = CommandEnvironment::fake();
        let first_batch = runner::scratch(&system).unwrap();
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let plan = build_plan(&reference_bundle(), &candidate).unwrap();
        let plans = [(plan.clone(), PathBuf::from("first")), (plan, PathBuf::from("second"))];
        let roots = execution::run_batch(
            &plans,
            first_batch.path(),
            &specgate_discovery::identity::PackageName::from("specgate-ctsc-fixtures"),
            |_plan, _out, root| Ok(root.to_path_buf()),
        )
        .unwrap();
        assert_eq!(roots, vec![first_batch.path().to_path_buf(); 2]);

        let second_batch = runner::scratch(&system).unwrap();
        assert_ne!(first_batch.path(), second_batch.path());
    }

    #[test]
    fn planner_mismatches() {
        let base = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        assert_error(&without_operation(&base), "missing operation");

        let mut duplicate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        Arc::make_mut(&mut duplicate.inner).raw_registry.ops.push(raw_operation("add"));
        assert_error(&duplicate, "duplicated");

        let mut renamed = normalized_operation("add");
        renamed.inputs[0].name = "left".into();
        assert_error(&candidate_metadata(raw_operation("add"), renamed), "input names differ");

        let mut reordered = normalized_operation("add");
        reordered.inputs.swap(0, 1);
        assert_error(&candidate_metadata(raw_operation("add"), reordered), "input order differs");

        let mut input_type = normalized_operation("add");
        input_type.inputs[0].ty = "i64".into();
        assert_error(&candidate_metadata(raw_operation("add"), input_type), "input 'a' type mismatch");

        let mut output_type = normalized_operation("add");
        output_type.output = Some("i64".into());
        assert_error(&candidate_metadata(raw_operation("add"), output_type), "output type mismatch");

        let mut asynchronous = raw_operation("add");
        asynchronous.is_async = true;
        assert_error(
            &candidate_metadata(asynchronous, normalized_operation("add")),
            "async metadata disagrees",
        );

        let mut method = raw_operation("add");
        method.is_method = true;
        assert_error(&candidate_metadata(method, normalized_operation("add")), "receiver");

        let mut private = raw_operation("add");
        private.is_public = false;
        assert_error(&candidate_metadata(private, normalized_operation("add")), "not public");

        let mut setup = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let mut setup_operation = raw_operation("add");
        setup_operation.is_setup = true;
        Arc::make_mut(&mut setup.inner).raw_registry.ops.push(setup_operation);
        assert_error(&setup, "ambiguously matches");
    }

    #[test]
    fn planner_selects_target_for_plain_test_operation() {
        let mut raw = raw_operation("add");
        let target: specgate_discovery::registry::TestTarget = serde_json::from_value(serde_json::json!({
            "name": "custom_library",
            "library": true,
        }))
        .unwrap();
        raw.test_target = Some(target.clone());
        let plan = build_plan(&reference_bundle(), &candidate_metadata(raw, normalized_operation("add"))).unwrap();
        assert_eq!(plan.test_target, Some(target));
    }

    #[test]
    fn planner_limits() {
        let mut bundle = reference_bundle();
        let operation = &bundle.registry.operations[0];
        let mut inputs = operation.inputs().to_vec();
        inputs[0].value_type = ReplayType::list(ReplayType::primitive("i32"));
        bundle.registry.operations[0] = replay_model::RegistryOp::builder(replay_model::OpDeps {
            component_id: operation.component_id().clone(),
            name: operation.name().clone(),
            inputs,
        })
        .output(operation.output().unwrap().clone())
        .build()
        .unwrap();
        bundle.scenarios[0].operations[0].inputs[0].value_type = bundle.registry.operations[0].inputs()[0].value_type.clone();
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let reason = build_plan(&bundle, &candidate).unwrap_err();
        assert!(reason.to_string().contains("unsupported structured type"));
        assert_eq!(classify(reason.to_string()), Some(ReplayLimitation::StructuredValue));
    }

    #[test]
    fn planner_checks_transitive_structured_outcome_declarations() {
        let (bundle, mut candidate) = structured_metadata();
        build_plan(&bundle, &candidate).expect("matching typed collection declarations");
        candidate.schema.types[0].fields[0].ty = "i64".into();
        let error = build_plan(&bundle, &candidate).unwrap_err();
        assert!(error.to_string().contains("Child"), "{error}");
        assert_eq!(classify(error.to_string()), Some(ReplayLimitation::StructuredValue));
    }

    #[test]
    fn planner_checks_nested_outcomes_without_scheduling_them() {
        let (mut bundle, mut candidate) = structured_metadata();
        let typed = &bundle.registry.operations[0];
        let nested = replay_model::RegistryOp::builder(replay_model::OpDeps {
            component_id: typed.component_id().clone(),
            name: replay_model::OperationName::try_new("inner").unwrap(),
            inputs: typed.inputs().to_vec(),
        })
        .output(typed.output().unwrap().clone())
        .build()
        .unwrap();
        bundle.registry.operations = vec![reference_bundle().registry.operations.remove(0), nested];
        let mut observed = bundle.scenarios[0].operations[0].clone();
        observed.operation_name = replay_model::OperationName::try_new("inner").unwrap();
        bundle.observed_operations.push(observed);
        let mut inner = candidate.schema.operations[0].clone();
        inner.name = "inner".into();
        candidate.schema.operations[0].output = Some("i32".into());
        candidate.schema.operations.push(inner);
        let schema = serde_json::to_string(&candidate.schema).unwrap();
        bundle.registry_document = specgate_ctsc::registry::encode(
            bundle.registry.identity.id.as_str(),
            bundle.registry.identity.version.as_str(),
            specgate_ctsc::registry::Schema::new(&schema),
        )
        .unwrap()
        .registry_json
        .into_bytes();
        let plan = build_plan(&bundle, &candidate).unwrap();
        assert_eq!(plan.links.len(), 1);
        assert_eq!(plan.links[0].operation_name.as_str(), "add");
        candidate.schema.types[0].fields[0].ty = "i64".into();
        let error = build_plan(&bundle, &candidate).unwrap_err();
        assert!(error.to_string().contains("Child"), "{error}");
    }

    #[test]
    fn malformed_structured_candidate_never_publishes() {
        use specgate::__rt::{self as capture, Value};
        let (bundle, candidate) = structured_metadata();
        let plan = build_plan(&bundle, &candidate).unwrap();
        let config = capture::Config::builder(capture::ConfigDeps {
            scenario_name: "add".into(),
            trace_id: "33333333333333333333333333333333".try_into().unwrap(),
            run_id: "3333333333333301".try_into().unwrap(),
            scenario_id: "3333333333333302".try_into().unwrap(),
        })
        .build()
        .unwrap();
        capture::start(config).unwrap();
        let mut operation = capture::begin_operation("fixture.stateless_add".into(), "add".into()).unwrap();
        operation.input("a", Value::Integer(2)).unwrap();
        operation.input("b", Value::Integer(3)).unwrap();
        operation
            .result(Value::List(vec![Value::Map(BTreeMap::from([(
                "child".to_string(),
                Value::Map(BTreeMap::from([("value".to_string(), Value::String("not an integer".to_string()))])),
            )]))]))
            .unwrap();
        let captures = [capture::finish().unwrap()];
        let system = CommandEnvironment::fake();
        system.seed_file("existing.otlp.json", b"original");
        for out in ["missing.otlp.json", "existing.otlp.json"] {
            let error = write(&plan, &captures, out, &system).unwrap_err();
            assert!(error.to_string().contains("failed linked validation"), "{error}");
        }
        assert!(!system.exists("missing.otlp.json"));
        assert_eq!(system.read("existing.otlp.json").unwrap(), b"original");
    }

    #[test]
    fn native_raw_identifiers_are_validated_and_preserved() {
        let mut raw = raw_operation("add");
        raw.module_path = "specgate_ctsc_fixtures::r#async".into();
        raw.fn_name = "r#type".into();
        let plan = build_plan(&reference_bundle(), &candidate_metadata(raw, normalized_operation("add"))).unwrap();
        assert!(source(&plan).contains("candidate::r#async::r#type("));
        for invalid in ["async", "r#self", "value; panic!()", "a::b", ""] {
            assert!(link::validate_ident(invalid, "test").is_err(), "{invalid}");
        }
    }

    #[test]
    fn module_paths_accept_only_library_or_reported_test_roots() {
        let mut raw = raw_operation("add");
        raw.test_target = Some(
            serde_json::from_value(serde_json::json!({
                "name": "environment-contract",
                "library": false,
            }))
            .unwrap(),
        );
        assert_eq!(link::module_path(&raw, "specgate_ctsc_fixtures").unwrap(), ["stateless"]);
        raw.module_path = "environment_contract::local".into();
        assert_eq!(link::module_path(&raw, "specgate_ctsc_fixtures").unwrap(), ["local"]);
        raw.module_path = "unrelated::local".into();
        assert!(
            link::module_path(&raw, "specgate_ctsc_fixtures")
                .unwrap_err()
                .to_string()
                .contains("does not begin with crate")
        );
        raw.test_target = None;
        raw.module_path = "environment_contract::local".into();
        link::module_path(&raw, "specgate_ctsc_fixtures").unwrap_err();
    }

    fn structured_metadata() -> (ReplayBundle, Candidate) {
        use specgate_discovery::schema::{Field, TypeDef, TypeKind};
        let mut operation = normalized_operation("add");
        operation.output = Some("List<Item>".into());
        let mut raw = raw_operation("add");
        raw.return_type = "Vec<Item>".into();
        let mut candidate = candidate_metadata(raw, operation);
        candidate.schema.types = vec![
            TypeDef {
                name: "Child".into(),
                kind: TypeKind::Struct,
                fields: vec![Field {
                    name: "value".into(),
                    ty: "i32".into(),
                }],
                variants: Vec::new(),
            },
            TypeDef {
                name: "Item".into(),
                kind: TypeKind::Struct,
                fields: vec![Field {
                    name: "child".into(),
                    ty: "Child".into(),
                }],
                variants: Vec::new(),
            },
        ];
        let schema = serde_json::to_string(&candidate.schema).unwrap();
        let mut bundle = reference_bundle();
        bundle.registry_document = specgate_ctsc::registry::encode(
            bundle.registry.identity.id.as_str(),
            bundle.registry.identity.version.as_str(),
            specgate_ctsc::registry::Schema::new(&schema),
        )
        .unwrap()
        .registry_json
        .into_bytes();
        let operation = &bundle.registry.operations[0];
        bundle.registry.operations[0] = replay_model::RegistryOp::builder(replay_model::OpDeps {
            component_id: operation.component_id().clone(),
            name: operation.name().clone(),
            inputs: operation.inputs().to_vec(),
        })
        .output(ReplayType::list(ReplayType::named("Item", replay_model::TypeOwner::default())))
        .build()
        .unwrap();
        (bundle, candidate)
    }

    #[test]
    fn limitation_categories() {
        for (reason, expected) in [
            (
                "candidate operation 'fixture.counter::increment' is setup-backed; replay does not yet construct setups",
                ReplayLimitation::SetupOperation,
            ),
            (
                "candidate operation 'fixture.counter::increment' is a method; replay supports only free functions",
                ReplayLimitation::MethodOperation,
            ),
            (
                "candidate operation 'fixture.fetch::fetch' is async; replay supports only synchronous operations",
                ReplayLimitation::AsyncOperation,
            ),
            (
                "replay currently supports only Rust candidates; implementation language is 'csharp'",
                ReplayLimitation::UnsupportedLanguage,
            ),
        ] {
            assert_eq!(classify(reason), Some(expected), "{reason}");
        }
        assert_eq!(
            classify("operation span '3' result uses unsupported structured replay type named"),
            Some(ReplayLimitation::StructuredValue)
        );
        assert_eq!(classify("candidate is missing operation 'fixture.add::add'"), None);
        assert_eq!(classify("capture bundle digest mismatch"), None);
    }

    #[test]
    fn error_chain_reports_causes() {
        let runner = Error::new_message(FailureStage::Linking, "candidate replay runner failed: no such file");
        let wrapped = Error::wrap(FailureStage::Execution, "candidate replay execution failed", runner);
        assert_eq!(
            wrapped.chain(),
            "candidate replay execution failed: candidate replay runner failed: no such file"
        );

        let repeated = Error::wrap(FailureStage::Capture, "bundle is unreadable", Error::from("bundle is unreadable"));
        assert_eq!(repeated.chain(), "bundle is unreadable");

        assert_eq!(Error::from("plain").chain(), "plain");
    }

    #[test]
    fn packaged_manifest() {
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let plan = build_plan(&reference_bundle(), &candidate).unwrap();
        let cargo = cargo_manifest(&plan).unwrap();
        let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
        assert_eq!(parsed["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
        assert_eq!(
            parsed["dependencies"]["specgate_runtime"]["path"].as_str(),
            Some("resolved-runtime")
        );
        assert_eq!(cargo.config, None);
    }

    #[test]
    #[should_panic(expected = "replay plan operation link index must reference a validated link")]
    fn corrupt_panics() {
        let candidate = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let mut plan = build_plan(&reference_bundle(), &candidate).unwrap();
        plan.scenarios[0].operations[0].link_index = usize::MAX;
        let _ = source(&plan);
    }

    fn reference_bundle() -> ReplayBundle {
        let i32_type = ReplayType::primitive("i32");
        let inputs = vec![
            ReplayInput {
                name: "a".to_string(),
                value_type: i32_type.clone(),
                value: ReplayValue::I32(2),
            },
            ReplayInput {
                name: "b".to_string(),
                value_type: i32_type.clone(),
                value: ReplayValue::I32(3),
            },
        ];
        ReplayBundle {
            registry_document: Vec::new(),
            observed_operations: Vec::new(),
            component_id: specgate_ctsc::replay::model::ComponentId::try_new("fixture.stateless_add").unwrap(),
            registry: replay_model::Registry {
                identity: replay_model::RegistryIdentity {
                    id: serde_json::from_str("\"urn:ctsc:registry:fixture.stateless_add\"").unwrap(),
                    version: serde_json::from_str("\"0.1.0\"").unwrap(),
                    digest: specgate_ctsc::replay::model::ArtifactDigest::try_new(format!("sha256:{}", "a".repeat(64))).unwrap(),
                },
                operations: vec![
                    replay_model::RegistryOp::builder(replay_model::OpDeps {
                        component_id: replay_model::ComponentId::try_new("fixture.stateless_add").unwrap(),
                        name: replay_model::OperationName::try_new("add").unwrap(),
                        inputs: inputs
                            .iter()
                            .map(|input| replay_model::RegistryInput {
                                name: input.name.clone(),
                                value_type: input.value_type.clone(),
                            })
                            .collect(),
                    })
                    .output(i32_type)
                    .build()
                    .unwrap(),
                ],
            },
            scenarios: vec![replay_model::Scenario {
                name: serde_json::from_str("\"add\"").unwrap(),
                index: specgate_ctsc::replay::model::ScenarioIndex::new(0).unwrap(),
                operations: vec![replay_model::Operation {
                    component_id: specgate_ctsc::replay::model::ComponentId::try_new("fixture.stateless_add").unwrap(),
                    operation_name: specgate_ctsc::replay::model::OperationName::try_new("add").unwrap(),
                    inputs,
                    output: Some(ReplayType::primitive("i32")),
                }],
            }],
        }
    }

    fn raw_operation(name: impl AsRef<str>) -> RawOperation {
        let name = name.as_ref();
        RawOperation {
            name: name.into(),
            module_path: "specgate_ctsc_fixtures::stateless".into(),
            fn_name: name.into(),
            type_arguments: Vec::new(),
            replay_output: None,
            test_target: None,
            is_setup: false,
            is_async: false,
            is_method: false,
            is_public: true,
            return_type: "i32".into(),
            fills: "".into(),
            params: vec![
                RawField {
                    name: "a".into(),
                    ty: "i32".into(),
                },
                RawField {
                    name: "b".into(),
                    ty: "i32".into(),
                },
            ],
            component: "fixture.stateless_add".into(),
            cs_class: None,
            cs_method_of: None,
            cs_method: None,
            cs_is_static: None,
            cs_return: None,
            cs_params: Vec::new(),
            cs_exceptions: specgate_discovery::registry::ExceptionMetadata::default(),
        }
    }

    fn normalized_operation(name: impl AsRef<str>) -> Operation {
        let name = name.as_ref();
        Operation {
            name: name.into(),
            is_async: false,
            inputs: vec![
                Input {
                    name: "a".into(),
                    ty: "i32".into(),
                },
                Input {
                    name: "b".into(),
                    ty: "i32".into(),
                },
            ],
            output: Some("i32".into()),
            empty: false,
            errors: Vec::new(),
            setups: Vec::new(),
        }
    }

    fn candidate_metadata(raw: RawOperation, normalized: Operation) -> Candidate {
        Candidate {
            inner: Arc::new(CandidatesInner {
                crate_name: "specgate_ctsc_fixtures".to_string(),
                target_name: "rust".into(),
                language: specgate_discovery::binding::Language::Rust,
                package_name: "specgate-ctsc-fixtures".into(),
                package_version: "0.1.0".into(),
                package_root: PathBuf::from("candidate"),
                runtime: specgate_discovery::runner::PackageSource::local("specgate-runtime", "0.6.0", "resolved-runtime"),
                executor_options: specgate_discovery::binding::LanguageOptions::Rust {
                    runtime: specgate_discovery::binding::Runtime::Auto,
                    executor: None,
                },
                executors: BTreeMap::new(),
                raw_registry: Registry {
                    ops: vec![raw],
                    types: Vec::new(),
                },
                schemas: HashMap::new(),
            }),
            schema: Schema {
                component: "fixture.stateless_add".into(),
                dependencies: Vec::new(),
                dependency_types: Vec::new(),
                operations: vec![normalized],
                types: Vec::new(),
            },
        }
    }

    fn without_operation(base: &Candidate) -> Candidate {
        let mut inner = (*base.inner).clone();
        inner.raw_registry.ops.clear();
        Candidate {
            inner: Arc::new(inner),
            schema: Schema {
                component: base.schema.component.clone(),
                dependencies: Vec::new(),
                dependency_types: Vec::new(),
                operations: Vec::new(),
                types: Vec::new(),
            },
        }
    }

    fn assert_error(candidate: &Candidate, expected: impl AsRef<str>) {
        let expected = expected.as_ref();
        let error = build_plan(&reference_bundle(), candidate).unwrap_err();
        assert!(error.to_string().contains(expected), "expected '{expected}' in '{error}'");
    }
}
