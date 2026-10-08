#![cfg(test)]

// Cohesive implementation group for this subsystem.

#[cfg(test)]
mod cases {
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
                "language: rust\ntargets:\n  default:\n    package_root: {}\n",
                cargo_path(&candidate)
            ),
        )
        .unwrap();
        (cache, binding)
    }

    #[test]
    fn replay_contract() {
        let capture_dir = output_dir("capture");
        let first_output = output_dir("first").with_extension("otlp.json");
        let second_output = output_dir("second").with_extension("otlp.json");
        let _ = std::fs::remove_dir_all(&capture_dir);
        let _ = std::fs::remove_file(&first_output);
        let _ = std::fs::remove_file(&second_output);

        let captured = capture(
            CaptureRequest::builder(CapturePaths {
                binding: rust_binding(),
                out: capture_dir.clone(),
            })
            .component(ComponentId::from("fixture.cli.replay"))
            .build()
            .unwrap(),
        );
        assert!(captured.is_ok(), "capture failed: {captured:?}");
        let component = specgate_discovery::identity::ComponentId::from("fixture.cli.replay");
        let candidates = Candidates::discover(rust_binding(), None, std::slice::from_ref(&component)).expect("focused candidate discovery");
        let candidates = Candidates::discover_with(
            &DiscoveryInput {
                binding: Path::new("unused"),
                target: None,
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
        assert!(candidate_text.contains("candidate:specgate-cli-fixtures:default"));
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
            replay(Request::builder(Paths::new(&capture_dir, csharp_binding(), &unsupported_output)).build().unwrap()),
            Err(error) if error.to_string().contains("only Rust candidates") && error.to_string().contains("csharp")
        ));
        assert!(!unsupported_output.exists());

        let _ = std::fs::remove_dir_all(capture_dir);
        let _ = std::fs::remove_file(first_output);
        let _ = std::fs::remove_file(second_output);
    }

    #[test]
    fn candidate_config() {
        let (cache, binding) = configured_candidate();
        let capture_dir = cache.path().join("capture");
        let replay_output = cache.path().join("candidate.otlp.json");
        let captured = capture(
            CaptureRequest::builder(CapturePaths {
                binding: binding.clone(),
                out: capture_dir.clone(),
            })
            .component(ComponentId::from("fixture.configured"))
            .build()
            .unwrap(),
        );
        assert!(captured.is_ok(), "capture failed: {captured:?}");
        let replayed = replay(
            Request::builder(Paths::new(&capture_dir, &binding, &replay_output))
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
        assert!(output.contains("candidate:configured-candidate:default"));
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
        assert_error(&candidate_metadata(asynchronous, normalized_operation("add")), "is async");

        let mut method = raw_operation("add");
        method.is_method = true;
        assert_error(&candidate_metadata(method, normalized_operation("add")), "is a method");

        let mut private = raw_operation("add");
        private.is_public = false;
        assert_error(&candidate_metadata(private, normalized_operation("add")), "not public");

        let mut setup = candidate_metadata(raw_operation("add"), normalized_operation("add"));
        let mut setup_operation = raw_operation("add");
        setup_operation.is_setup = true;
        Arc::make_mut(&mut setup.inner).raw_registry.ops.push(setup_operation);
        assert_error(&setup, "setup-backed");
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
                "replay currently supports only Rust candidates; binding language is 'csharp'",
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
                target_name: "default".into(),
                language: specgate_discovery::binding::Language::Rust,
                package_name: "specgate-ctsc-fixtures".into(),
                package_version: "0.1.0".into(),
                package_root: PathBuf::from("candidate"),
                runtime: specgate_discovery::runner::PackageSource::local("specgate-runtime", "0.6.0", "resolved-runtime"),
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
