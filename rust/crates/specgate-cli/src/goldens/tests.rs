#![cfg(test)]

// Cohesive implementation group for this subsystem.
use super::*;

/// Regenerate or verify the whole CTSC golden matrix.
///
/// Ignored by default because it drives the real Rust and .NET toolchains.
/// `just ctsc-goldens-update` and `just ctsc-goldens-check` select the mode
/// through `SPECGATE_CTSC_GOLDENS`.
#[test]
#[ignore = "drives the real Rust and .NET toolchains; run through just ctsc-goldens-check"]
pub(super) fn golden_matrix() {
    let mode = match std::env::var("SPECGATE_CTSC_GOLDENS").unwrap_or_default().as_str() {
        "update" => Mode::Update,
        "check" | "" => Mode::Check,
        other => panic!("SPECGATE_CTSC_GOLDENS must be 'update' or 'check', got '{other}'"),
    };
    run(mode).expect("golden matrix I/O");
}

#[cfg(test)]
mod cases {
    use super::*;

    #[test]
    fn path_roundtrip() {
        for value in ["test/goldens/ctsc", "a-b/c_d/file.json", "single"] {
            let path = RepoPath::try_from(value).expect("portable path");
            assert_eq!(path.as_str(), value);
            assert_eq!(serde_json::to_string(&path).unwrap(), format!("\"{value}\""));
            assert_eq!(serde_json::from_str::<RepoPath>(&format!("\"{value}\"")).unwrap(), path);
        }
    }

    #[test]
    fn invalid_paths() {
        for value in [
            "",
            "/absolute",
            "//server/share",
            "C:/absolute",
            "c:relative",
            r"C:\absolute",
            r"\\server\share",
            r"\\?\C:\absolute",
            r"\\.\device",
            "a\\b",
            ".",
            "..",
            "a/./b",
            "a/../b",
            "a//b",
            "a/",
        ] {
            let error = RepoPath::try_from(value).expect_err(value);
            assert!(
                error.to_string().contains("not a portable repository-relative path"),
                "{value}: {error}"
            );
        }
    }

    #[test]
    fn enum_diagnostics() {
        let phase = serde_json::from_str::<Phase>(r#""deploy""#).unwrap_err().to_string();
        assert!(phase.contains("unknown variant `deploy`"), "{phase}");
        assert!(
            phase.contains("discover") && phase.contains("capture") && phase.contains("build"),
            "{phase}"
        );

        let mode = serde_json::from_str::<ReplayMode>(r#""sometimes""#).unwrap_err().to_string();
        assert!(mode.contains("unknown variant `sometimes`"), "{mode}");
        assert!(mode.contains("verified") && mode.contains("unsupported"), "{mode}");
    }

    #[test]
    pub(super) fn covers_sources() {
        let root = repo_root(std::env::current_dir().expect("current directory"));
        let matrix = load_matrix(&root);
        check_shape(&matrix);
        check_coverage(&root, &matrix).expect("golden sources should be covered");
        check_replacements(&root, &matrix).unwrap_or_else(|error| panic!("replacement validation failed: {error}"));
    }

    #[test]
    pub(super) fn negative_categories() {
        assert_eq!(
            error_category(
                "operation 'fixture.duplicate_identity::render' is declared 2 times; operation identity must be unique within a component"
            )
            .as_str(),
            "duplicate-operation-identity"
        );
        assert_eq!(
            error_category(
                "setup 'make_counter' for 'fixture.missing_operation::increment' has no operation to construct; annotate the operation or remove the setup"
            )
            .as_str(),
            "orphan-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.missing_setup::increment' is a method with no receiver setup; annotate a #[spec_setup(\"increment\")] producer for its receiver"
            )
            .as_str(),
            "method-missing-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.private_operation::secret' is declared on private function 'secret'; discovery exposes only public operations"
            )
            .as_str(),
            "private-operation"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.value::echo' output type 'value' is the dynamic runtime value; CTSC registries require declared semantic types"
            )
            .as_str(),
            "dynamic-value-type"
        );
        assert_eq!(
            error_category("referenced type 'Gadget' has no registered component owner").as_str(),
            "unresolved-type"
        );
        assert_eq!(error_category("something else entirely").as_str(), "unclassified");
    }

    #[test]
    pub(super) fn difference_paths() {
        let rust = serde_json::json!({
            "components": [
                { "id": "comp.app", "dependencies": [{ "componentId": "comp.core" }], "types": [] },
                { "id": "comp.core", "types": [{ "name": "Widget" }] }
            ]
        });
        let csharp = serde_json::json!({
            "components": [
                { "id": "comp.app", "types": [{ "name": "Widget" }] }
            ]
        });

        let differences = semantic_differences(&rust, &csharp);
        assert_eq!(
            differences.iter().map(|difference| difference.path.clone()).collect::<Vec<_>>(),
            vec![
                "/components".to_string(),
                "/components/0/dependencies".to_string(),
                "/components/0/types".to_string(),
            ],
            "array length first, then the common prefix element-wise, in pointer order"
        );
        assert_eq!(differences[0].rust, ParitySide::ArrayLength { length: 2 });
        assert_eq!(differences[0].csharp, ParitySide::ArrayLength { length: 1 });
        assert_eq!(differences[1].csharp, ParitySide::Missing);
        assert_eq!(differences[2].rust, ParitySide::ArrayLength { length: 0 });

        assert!(
            semantic_differences(&rust, &rust).is_empty(),
            "identical documents have no semantic difference"
        );
        assert_eq!(
            semantic_differences(&serde_json::json!({ "kind": "tuple" }), &serde_json::json!({ "kind": "record" })),
            vec![ParityDifference {
                path: "/kind".to_string(),
                rust: ParitySide::Value {
                    value: serde_json::json!("tuple")
                },
                csharp: ParitySide::Value {
                    value: serde_json::json!("record")
                },
            }]
        );
        assert_eq!(
            semantic_differences(&serde_json::json!({ "a/b": 1 }), &serde_json::json!({ "a/b": 2 }))[0].path,
            "/a~1b",
            "pointer tokens are escaped"
        );
    }

    #[test]
    pub(super) fn parity_exceptions() {
        let root = repo_root(std::env::current_dir().expect("current directory"));
        let matrix = load_matrix(&root);
        let exceptions = matrix
            .rows
            .iter()
            .filter(|row| row.parity.mode == ParityMode::Exception)
            .collect::<Vec<_>>();
        assert_eq!(
            exceptions
                .iter()
                .map(|row| (row.component.as_deref().unwrap(), row.parity.allowed_differences.len()))
                .collect::<Vec<_>>(),
            [("comp.app", 4), ("fixture.enum_event", 3), ("fixture.scalar_types", 6)]
        );
        // Actual native differences are checked against this exact set by the
        // full matrix gate; no second checked-in producer registry is needed.
    }

    #[test]
    fn canonical_registry_anchor_rejects_simultaneous_drift() {
        let mut matrix = check_matrix(serde_json::json!([
            {"reference": "rust", "candidate": "other", "mode": "verified"}
        ]));
        let row = &mut matrix.rows[0];
        row.csharp = Some(
            synthetic_row("fixture.other", [Phase::Discover, Phase::Capture], &serde_json::json!([]))
                .rust
                .unwrap(),
        );
        row.csharp.as_mut().unwrap().binding = "other".to_string();
        row.parity.mode = ParityMode::ByteIdentical;
        let root = repo_root(std::env::current_dir().unwrap());
        let original = read_bytes(root.join("test/goldens/ctsc/component/fixture.stateless_add/registry.ctsc.json"));
        let scratch = tempfile::tempdir().unwrap();
        let generated = scratch.path().join("generated");
        let stored = scratch.path().join("stored");
        write_bytes(row.reference_dir(&stored).join(registry_file()), &original).unwrap();
        for (_, endpoint) in row.implementations() {
            write_bytes(row.producer_registry(&generated, scratch.path(), &endpoint.binding), &original).unwrap();
        }
        check_parity(&matrix, &generated, scratch.path(), &stored);
        let mut drifted = original.clone();
        drifted.push(b'\n'); // Valid full registry, semantically unchanged, different native bytes.
        for (_, endpoint) in matrix.rows[0].implementations() {
            write_bytes(
                matrix.rows[0].producer_registry(&generated, scratch.path(), &endpoint.binding),
                &drifted,
            )
            .unwrap();
        }
        let failure = std::panic::catch_unwind(|| check_parity(&matrix, &generated, scratch.path(), &stored)).unwrap_err();
        assert!(
            failure
                .downcast_ref::<String>()
                .unwrap()
                .contains("selected native registry differs from canonical golden")
        );
        assert!(!registry_parity_problems(&matrix.rows[0], "rust", &drifted, &original).is_empty());
    }

    #[test]
    fn exact_registry_exceptions_preserve_language_orientation() {
        let mut row = synthetic_row("fixture.parity", [Phase::Discover], &serde_json::json!([]));
        row.rust.as_mut().unwrap().binding = "candidate-v2".to_string();
        row.csharp = Some(
            synthetic_row("fixture.parity", [Phase::Discover], &serde_json::json!([]))
                .rust
                .unwrap(),
        );
        row.csharp.as_mut().unwrap().binding = "baseline-v1".to_string();
        row.parity.mode = ParityMode::Exception;
        let rust = serde_json::json!({"kind": "tuple", "items": ["i64"]});
        let csharp = serde_json::json!({"kind": "record", "fields": [{"name": "value", "type": "i64"}]});
        row.parity.allowed_differences = semantic_differences(&rust, &csharp);
        let rust = serde_json::to_vec(&rust).unwrap();
        let csharp = serde_json::to_vec(&csharp).unwrap();
        for (selected, other, canonical, producer) in [
            ("baseline-v1", "candidate-v2", &csharp, &rust),
            ("candidate-v2", "baseline-v1", &rust, &csharp),
        ] {
            row.reference.as_mut().unwrap().implementation = selected.to_string();
            assert!(registry_parity_problems(&row, other, producer, canonical).is_empty());
            assert!(registry_parity_problems(&row, selected, canonical, canonical).is_empty());
            let stale = registry_parity_problems(&row, other, canonical, canonical);
            assert!(stale.iter().any(|problem| problem.contains("stale")));
            let mut changed: serde_json::Value = serde_json::from_slice(producer).unwrap();
            changed["new"] = serde_json::json!("difference");
            assert!(
                registry_parity_problems(&row, other, &serde_json::to_vec(&changed).unwrap(), canonical)
                    .iter()
                    .any(|problem| problem.contains("undeclared"))
            );
            let mut missing = row.parity.allowed_differences.pop().unwrap();
            assert!(!registry_parity_problems(&row, other, producer, canonical).is_empty());
            missing.path.push_str("/changed");
            row.parity.allowed_differences.push(missing);
            assert!(
                registry_parity_problems(&row, other, producer, canonical)
                    .iter()
                    .any(|problem| problem.contains("no longer occurs"))
            );
            row.parity.allowed_differences =
                semantic_differences(&serde_json::from_slice(&rust).unwrap(), &serde_json::from_slice(&csharp).unwrap());
        }
    }

    #[test]
    fn discovery_only_component_has_one_canonical_registry() {
        let matrix = async_matrix([Phase::Discover]);
        check_shape(&matrix);
        assert_eq!(
            expected_artifacts(&matrix)
                .into_iter()
                .map(|path| path.to_string())
                .collect::<Vec<_>>(),
            ["component/fixture.async_setup/registry.ctsc.json"]
        );
        let row = &matrix.rows[0];
        let scratch = tempfile::tempdir().unwrap();
        let generated = scratch.path().join("generated");
        let root = repo_root(std::env::current_dir().unwrap());
        let bytes = read_bytes(root.join("test/goldens/ctsc/component/fixture.stateless_add/registry.ctsc.json"));
        publish_discovery_registry(row, "rust", &generated, scratch.path(), &bytes).unwrap();
        assert_eq!(read_bytes(row.reference_dir(&generated).join(registry_file())), bytes);
        assert_eq!(read_bytes(row.producer_registry(&generated, scratch.path(), "rust")), bytes);
        assert_eq!(actual_artifacts(&generated).unwrap(), expected_artifacts(&matrix));
        validate_artifacts(&matrix, &generated);
        check_parity(&matrix, &generated, scratch.path(), &generated);
    }

    #[test]
    fn csharp_discovery_only_command_binding_generates_registry_without_capture() {
        let root = repo_root(std::env::current_dir().unwrap());
        let scratch = tempfile::tempdir().unwrap();
        let generated = scratch.path().join("generated");
        let work = scratch.path().join("work");
        let sources = root.join("rust/crates/specgate-cli/tests/fixtures/csharp");
        let mut matrix = load_matrix(&root);
        matrix.binding = RepoPath::try_from("binding.json").unwrap();
        let mut components = serde_json::Map::new();
        matrix.rows = ["fixture.cli.replay", "fixture.cli.multiple", "fixture.cli.setup"]
            .into_iter()
            .map(|component| {
                components.insert(
                    component.to_string(),
                    serde_json::json!({
                        "implementations": {
                            "baseline-v1": {
                                "language": "csharp",
                                "sources": sources,
                                // Valid discovery configuration, unsupported by capture.
                                // Neither capture nor this command may run in this batch.
                                "tests": { "command": "this-command-must-not-be-run" }
                            }
                        }
                    }),
                );
                let mut row = synthetic_row(component, [Phase::Discover], &serde_json::json!([]));
                row.csharp = row.rust.take();
                row.csharp.as_mut().unwrap().binding = "baseline-v1".to_string();
                row.reference.as_mut().unwrap().implementation = "baseline-v1".to_string();
                row.parity.mode = ParityMode::NotApplicable;
                row.parity.reason = Some("Single discovery-only C# endpoint.".to_string());
                row
            })
            .collect();
        write_bytes(
            scratch.path().join("binding.json"),
            serde_json::to_vec(&serde_json::json!({ "components": components })).unwrap(),
        )
        .unwrap();
        check_shape(&matrix);
        generate_csharp(scratch.path(), &matrix, &generated, &work).expect("C# discovery-only generation must not require capture");
        validate_artifacts(&matrix, &generated);
        check_parity(&matrix, &generated, &work, &generated);
        verify_replay(scratch.path(), &matrix, &generated, &work).unwrap();
        assert_eq!(actual_artifacts(&generated).unwrap(), expected_artifacts(&matrix));
        assert_eq!(expected_artifacts(&matrix).len(), 3, "one canonical registry per component");
        assert!(!work.join("producer-captures").exists(), "discovery must not invoke native capture");
        for row in &matrix.rows {
            let registry: serde_json::Value =
                serde_json::from_slice(&read_bytes(row.reference_dir(&generated).join(registry_file()))).unwrap();
            assert_eq!(registry["components"][0]["id"], row.component.as_deref().unwrap());
            assert_eq!(
                read_bytes(row.reference_dir(&generated).join(registry_file())),
                read_bytes(row.producer_registry(&generated, &work, "baseline-v1"))
            );
        }
        assert!(
            actual_artifacts(&work)
                .unwrap()
                .iter()
                .all(|path| Path::new(path.as_str()).file_name() == Some(registry_file().as_os_str())),
            "discovery-only batches produce no capture or replay files"
        );
    }

    #[test]
    fn successful_endpoint_schema_rejects_any_legacy_inventory() {
        for artifacts in [
            serde_json::json!([]),
            serde_json::json!(["registry.ctsc.json"]),
            serde_json::json!(["manifest.json", "reference.otlp.json"]),
        ] {
            let error = serde_json::from_value::<LanguageRow>(serde_json::json!({
                "binding": "baseline-v1", "phases": ["discover"], "expect": "success", "artifacts": artifacts
            }))
            .unwrap_err();
            assert!(error.to_string().contains("unknown field `artifacts`"), "{error}");
        }
    }

    #[test]
    fn canonical_registry_preserves_full_dependency_closure_and_order() {
        for (component, dependency) in [("comp.app", "comp.core"), ("fixture.extract", "fixture.named_inputs")] {
            let raw = serde_json::json!({
                "operations": [{
                    "name": "get", "module_path": "fixture", "fn_name": "get",
                    "is_setup": false, "is_async": false, "is_method": false,
                    "is_public": true, "return_type": "Shared", "fills": "",
                    "params": [], "component": component
                }],
                "types": [{
                    "name": "Shared", "module_path": "fixture", "kind": "struct",
                    "component": dependency, "fields": [["value", "i32"]], "variants": []
                }]
            });
            let registry = Registry::parse(raw.to_string()).unwrap();
            let schema = normalize_registry(&registry, specgate_discovery::binding::Language::Rust, component).unwrap();
            let bytes = registry_bytes(&schema, component).unwrap();
            let document: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                document["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value["id"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                [component, dependency]
            );
            assert_eq!(document["components"][1]["types"][0]["name"], "Shared");
            let row = synthetic_row(component, [Phase::Discover], &serde_json::json!([]));
            let scratch = tempfile::tempdir().unwrap();
            let generated = scratch.path().join("generated");
            publish_discovery_registry(&row, "rust", &generated, scratch.path(), &bytes).unwrap();
            assert_eq!(read_bytes(row.reference_dir(&generated).join(registry_file())), bytes);
            let mut projected = document.clone();
            projected["components"].as_array_mut().unwrap().pop();
            assert!(!registry_parity_problems(&row, "rust", &serde_json::to_vec(&projected).unwrap(), &bytes).is_empty());
            let mut reordered = document;
            reordered["components"].as_array_mut().unwrap().reverse();
            assert!(!registry_parity_problems(&row, "rust", &serde_json::to_vec(&reordered).unwrap(), &bytes).is_empty());
        }
    }

    #[test]
    pub(super) fn build_negatives() {
        let intentional = BTreeSet::from([RepoPath::try_from(
            "test/rust/negative-fixtures/specgate-ctsc-negative-fixtures/src/templates/compile_error.rs.in",
        )
        .expect("fixture path")]);
        let blamed = r#"{"reason":"compiler-message","message":{"level":"error","code":null,"spans":[{"is_primary":true,"file_name":"src\\templates/compile_error.rs.in"}]}}"#;
        assert_eq!(
            compile_errors(blamed, &intentional).expect("the intentional source is blamed"),
            CompilerRejection {
                sources: vec![RepoPath::try_from("compile_error.rs.in").expect("source path")],
                codes: Vec::new(),
            }
        );

        let coded = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0425"},"spans":[{"is_primary":true,"file_name":"src/templates/compile_error.rs.in"}]}}"#;
        assert_eq!(
            compile_errors(coded, &intentional).expect("codes are recorded when present").codes,
            vec!["E0425".to_string()]
        );

        let unrelated = r#"{"reason":"compiler-message","message":{"level":"error","code":null,"spans":[{"is_primary":true,"file_name":"src/lib.rs"}]}}"#;
        let error = compile_errors(unrelated, &intentional).expect_err("an unrelated error is not this negative");
        assert!(error.to_string().contains("src/lib.rs"), "{error}");

        let mixed = format!("{coded}\n{unrelated}");
        let error = compile_errors(&mixed, &intentional).expect_err("one intended error may not hide an unrelated compiler error");
        assert!(error.to_string().contains("not exclusively attributable"), "{error}");
        assert!(error.to_string().contains("src/lib.rs"), "{error}");

        let unspanned = r#"{"reason":"compiler-message","message":{"level":"error","code":{"code":"E0999"},"spans":[]}}"#;
        let mixed_unspanned = format!("{coded}\n{unspanned}");
        let error = compile_errors(&mixed_unspanned, &intentional)
            .expect_err("an unspanned compiler error cannot be attributed to the intentional source");
        assert!(error.to_string().contains("errors without primary spans: 1"), "{error}");

        let warning_only = r#"{"reason":"compiler-message","message":{"level":"warning","code":null,"spans":[{"is_primary":true,"file_name":"src/templates/compile_error.rs.in"}]}}"#;
        let error = compile_errors(warning_only, &intentional).expect_err("a warning is not a rejection");
        assert!(error.to_string().contains("not a compiler diagnostic"), "{error}");
        assert!(
            compile_errors("", &intentional).is_err(),
            "a build that produced no diagnostics at all cannot be this negative"
        );
    }

    #[test]
    pub(super) fn inventory_errors() {
        let expected = BTreeSet::from(["fixture.one".to_string(), "fixture.two".to_string()]);
        assert!(inventory_mismatch("label", &expected, &["fixture.one".to_string(), "fixture.two".to_string()]).is_none());

        let missing = inventory_mismatch("label", &expected, &["fixture.one".to_string()])
            .expect("a component the matrix declares but discovery lacks must fail");
        assert!(missing.contains("absent from discovery: [fixture.two]"));

        let extra = inventory_mismatch(
            "label",
            &expected,
            &["fixture.one".to_string(), "fixture.two".to_string(), "fixture.three".to_string()],
        )
        .expect("a compiled component no row declares must fail");
        assert!(extra.contains("declared by no matrix row: [fixture.three]"));
    }

    #[test]
    pub(super) fn capture_coverage() {
        let operation = |name: &str| OperationIdentity::new("fixture.two_operations", name.to_string());
        let discovered = BTreeSet::from([operation("outer"), operation("inner")]);
        let captured = BTreeSet::from([operation("outer")]);
        let problems = coverage_problems("component/fixture.two_operations", &discovered, &captured, &BTreeSet::new());
        assert_eq!(
            problems,
            vec![
                "component/fixture.two_operations: discovered operations have no captured trace: \
                 [fixture.two_operations::inner]"
                    .to_string()
            ]
        );

        let nested_capture = BTreeSet::from([operation("outer"), operation("inner")]);
        assert!(
            coverage_problems("component/fixture.two_operations", &discovered, &nested_capture, &BTreeSet::new()).is_empty(),
            "a nested operation span exercises the semantic operation"
        );

        let excluded = BTreeSet::from([operation("inner")]);
        assert!(
            coverage_problems("component/fixture.two_operations", &discovered, &captured, &excluded).is_empty(),
            "an exact operation-level exclusion covers the one intentional gap"
        );
        assert!(
            coverage_problems("component/fixture.two_operations", &discovered, &nested_capture, &excluded)[0]
                .contains("capture exclusions are stale"),
            "an exclusion fails once the operation is captured"
        );
    }

    pub(super) fn synthetic_row(component: impl AsRef<str>, phases: impl AsRef<[Phase]>, limitations: &serde_json::Value) -> Row {
        let component = component.as_ref();
        let phases = phases.as_ref();
        let capturing = phases.contains(&Phase::Capture);
        let reference = serde_json::json!({
            "implementation": "rust",
            "artifacts": if capturing { vec![manifest_file(), trace_file(), registry_file()] } else { vec![registry_file()] }
        });
        let checks = if capturing {
            serde_json::json!([{ "reference": "rust", "candidate": "rust", "mode": "verified" }])
        } else {
            serde_json::json!([])
        };
        let linkage = if capturing {
            serde_json::json!({ "mode": "linked" })
        } else {
            serde_json::Value::Null
        };
        serde_json::from_value(serde_json::json!({
            "id": format!("component/{component}"),
            "classification": CLASS_COMPONENT,
            "sourceGroup": "synthetic",
            "component": component,
            "sources": ["test/rust/crates/specgate-ctsc-fixtures/src/synthetic.rs"],
            "rust": {
                "binding": "rust",
                "phases": phases,
                "expect": "success",
            },
            "reference": reference,
            "parity": { "mode": "rust-only", "reason": "Synthetic row used only by this test." },
            "checks": checks,
            "linkage": linkage,
            "limitations": limitations,
        }))
        .expect("synthetic row deserializes")
    }

    fn check_matrix(checks: serde_json::Value) -> Matrix {
        let mut matrix = async_matrix([Phase::Discover, Phase::Capture]);
        matrix.rows[0].limitations.clear();
        matrix.rows[0].checks = serde_json::from_value(checks).expect("check declarations");
        matrix
    }

    fn shape_failure(matrix: &Matrix, expected: &str) {
        let failure = std::panic::catch_unwind(|| check_shape(matrix)).expect_err("invalid declaration must fail before execution");
        let diagnostic = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .expect("string diagnostic");
        assert!(diagnostic.contains(expected), "expected '{expected}', got '{diagnostic}'");
    }

    #[test]
    fn directed_check_declarations() {
        let verified = serde_json::json!({"reference": "rust", "candidate": "rust", "mode": "verified"});
        check_shape(&check_matrix(serde_json::json!([verified])));
        shape_failure(&check_matrix(serde_json::json!([])), "missing checks");
        shape_failure(&check_matrix(serde_json::json!([verified, verified])), "duplicate check endpoints");
        for (field, value, expected) in [
            ("reference", serde_json::json!("unknown"), "unknown reference implementation"),
            ("candidate", serde_json::json!("unknown"), "unknown candidate implementation"),
            (
                "expectCategory",
                serde_json::json!("structured-value"),
                "stale replay expectCategory",
            ),
        ] {
            let mut check = verified.clone();
            check[field] = value;
            shape_failure(&check_matrix(serde_json::json!([check])), expected);
        }
        let mut no_capture = check_matrix(serde_json::json!([verified]));
        no_capture.rows[0].rust.as_mut().unwrap().phases = vec![Phase::Discover];
        shape_failure(&no_capture, "discovery-only component owns no checks");
        let mut unavailable = check_matrix(serde_json::json!([verified]));
        unavailable.rows[0].rust.as_mut().unwrap().phases = vec![Phase::Capture];
        shape_failure(&unavailable, "has no successful discovery");
    }

    #[test]
    fn unsupported_declarations_are_exact() {
        let check = serde_json::json!({
            "reference": "rust", "candidate": "rust", "mode": "unsupported",
            "reason": "Structured projection is unsupported.", "expectCategory": "structured-value"
        });
        check_shape(&check_matrix(serde_json::json!([check])));
        let mut missing = check.clone();
        missing.as_object_mut().unwrap().remove("expectCategory");
        shape_failure(&check_matrix(serde_json::json!([missing])), "must declare expectCategory");
        let mut unknown = check;
        unknown["expectCategory"] = serde_json::json!("anything");
        shape_failure(&check_matrix(serde_json::json!([unknown])), "unknown replay failure category");
        let reason = "operation 'fixture::echo' uses unsupported structured replay type 'Record'";
        assert!(unsupported_problem("check", "structured-value", reason).is_none());
        assert!(
            unsupported_problem("check", "method-operation", reason)
                .unwrap()
                .contains("but got 'structured-value'")
        );
        assert!(
            unsupported_problem("check", "structured-value", "bundle digest mismatch")
                .unwrap()
                .contains("unrelated, unclassified")
        );
    }

    #[test]
    fn implementation_identities_are_not_languages() {
        let mut matrix = check_matrix(serde_json::json!([
            {"reference": "baseline-v1", "candidate": "candidate-v2", "mode": "verified"}
        ]));
        let row = &mut matrix.rows[0];
        row.rust.as_mut().unwrap().binding = "candidate-v2".to_string();
        row.rust.as_mut().unwrap().phases = vec![Phase::Discover];
        let mut reference = synthetic_row("fixture.reference", [Phase::Discover, Phase::Capture], &serde_json::json!([]))
            .rust
            .unwrap();
        reference.binding = "baseline-v1".to_string();
        row.csharp = Some(reference);
        row.reference.as_mut().unwrap().implementation = "baseline-v1".to_string();
        row.parity.mode = ParityMode::ByteIdentical;
        row.parity.reason = None;
        check_shape(&matrix);
        assert!(matrix.rows[0].implementation_dir("out", "baseline-v1").ends_with("csharp"));
        assert_eq!(
            matrix.rows[0].capture_dir(Path::new("out"), Path::new("scratch"), "baseline-v1"),
            matrix.rows[0].reference_dir("out")
        );
        assert_eq!(
            matrix.rows[0].reference_dir("out"),
            Path::new("out").join(&matrix.rows[0].id),
            "endpoint identities resolve the same flat component oracle"
        );
        assert!(matrix.rows[0].captures_any());
        assert!(
            !matrix.rows[0].captures_rust(),
            "C#-only capture must still require linked validation"
        );
        matrix.rows[0].csharp.as_mut().unwrap().binding = "candidate-v2".to_string();
        shape_failure(&matrix, "duplicate implementation endpoints");
    }

    #[test]
    fn reference_selection_and_inventory() {
        let valid = || {
            check_matrix(serde_json::json!([
                {"reference": "rust", "candidate": "rust", "mode": "verified"}
            ]))
        };
        let mut matrix = valid();
        matrix.rows[0].reference = None;
        shape_failure(&matrix, "must select a reference");
        let mut matrix = valid();
        matrix.rows[0].reference.as_mut().unwrap().implementation = "unknown".to_string();
        shape_failure(&matrix, "unknown selected reference implementation");
        for artifacts in [
            serde_json::json!(["manifest.json", "reference.otlp.json"]),
            serde_json::json!(["manifest.json", "reference.otlp.json", "registry.ctsc.json", "candidate.otlp.json"]),
            serde_json::json!(["manifest.json", "manifest.json", "registry.ctsc.json"]),
            serde_json::json!(["rust/manifest.json", "reference.otlp.json", "registry.ctsc.json"]),
        ] {
            let mut matrix = valid();
            matrix.rows[0].reference.as_mut().unwrap().artifacts = serde_json::from_value(artifacts).unwrap();
            shape_failure(&matrix, "wrong reference artifact set");
        }
        let mut matrix = valid();
        matrix.rows[0].csharp = Some(
            synthetic_row("fixture.other", [Phase::Discover, Phase::Capture], &serde_json::json!([]))
                .rust
                .unwrap(),
        );
        matrix.rows[0].csharp.as_mut().unwrap().binding = "other".to_string();
        matrix.rows[0].checks[0].reference = "other".to_string();
        shape_failure(&matrix, "does not equal selected reference");
        // Both producers still capture; only the selected one needs a check.
        matrix.rows[0].checks[0].reference = "rust".to_string();
        check_shape(&matrix);
        assert!(
            matrix.rows[0]
                .capture_dir(Path::new("out"), Path::new("scratch"), "other")
                .starts_with(Path::new("scratch").join("producer-captures"))
        );
        for classification in [Classification::NegativeFixture, Classification::Replacement] {
            let mut matrix = valid();
            matrix.rows[0].classification = classification;
            shape_failure(&matrix, "only component rows may select a reference");
        }
        let mut matrix = valid();
        matrix.rows[0].rust.as_mut().unwrap().phases = vec![Phase::Discover];
        matrix.rows[0].checks.clear();
        let mut producer = synthetic_row("fixture.other", [Phase::Discover, Phase::Capture], &serde_json::json!([]))
            .rust
            .unwrap();
        producer.binding = "other".to_string();
        matrix.rows[0].csharp = Some(producer);
        shape_failure(&matrix, "has no successful capture");
    }

    #[test]
    fn artifact_inventory_rejects_legacy_bundles_and_candidate_leaks() {
        let matrix = check_matrix(serde_json::json!([
            {"reference": "rust", "candidate": "rust", "mode": "verified"}
        ]));
        let scratch = tempfile::tempdir().unwrap();
        let row = &matrix.rows[0];
        assert_eq!(
            expected_artifacts(&matrix)
                .into_iter()
                .map(|path| path.to_string())
                .collect::<Vec<_>>(),
            [
                "component/fixture.async_setup/manifest.json",
                "component/fixture.async_setup/reference.otlp.json",
                "component/fixture.async_setup/registry.ctsc.json",
            ]
        );
        for artifact in expected_artifacts(&matrix) {
            write_bytes(scratch.path().join(artifact), b"inventory only").unwrap();
        }
        assert_eq!(actual_artifacts(scratch.path()).unwrap(), expected_artifacts(&matrix));
        for name in [
            "rust/registry.ctsc.json",
            "csharp/registry.ctsc.json",
            "rust/manifest.json",
            "rust/reference.otlp.json",
            "csharp/manifest.json",
            "csharp/reference.otlp.json",
            "reference/registry.ctsc.json",
            "reference/manifest.json",
            "reference/reference.otlp.json",
            "reference/candidate.otlp.json",
            "candidate.otlp.json",
        ] {
            let leak = scratch.path().join(&row.id).join(name);
            write_bytes(&leak, b"unexpected").unwrap();
            let found = actual_artifacts(scratch.path()).unwrap();
            assert_eq!(found.difference(&expected_artifacts(&matrix)).count(), 1, "{name}");
            std::fs::remove_file(leak).unwrap();
        }
    }

    #[test]
    fn nonreference_capture_is_validated_and_covered_independently() {
        let root = repo_root(std::env::current_dir().unwrap());
        let mut matrix = check_matrix(serde_json::json!([
            {"reference": "rust", "candidate": "rust", "mode": "verified"}
        ]));
        let row = &mut matrix.rows[0];
        row.csharp = Some(
            synthetic_row("fixture.other", [Phase::Discover, Phase::Capture], &serde_json::json!([]))
                .rust
                .unwrap(),
        );
        row.csharp.as_mut().unwrap().binding = "other".to_string();
        let scratch = tempfile::tempdir().unwrap();
        let out = scratch.path().join("generated");
        let producer = row.capture_dir(&out, scratch.path(), "other");
        // A real native bundle in nonselected scratch must be validated and
        // retain its own registry, even if the selected reference is broken.
        let source = root.join("test/goldens/ctsc/component/fixture.stateless_add");
        for file in [manifest_file(), trace_file(), registry_file()] {
            write_bytes(producer.join(file), read_bytes(source.join(file))).unwrap();
        }
        write_bytes(row.reference_dir(&out).join(registry_file()), b"not this producer's registry").unwrap();
        validate_producer_capture(row, "other", &out, scratch.path()).unwrap();
        assert!(
            !row.implementation_dir(&out, "other").exists(),
            "no successful per-implementation goldens"
        );
        let operation = OperationIdentity::new("fixture.stateless_add", "add");
        let discovered = BTreeSet::from([operation.clone(), OperationIdentity::new("fixture.stateless_add", "missing")]);
        let problems = row_coverage("other", row, &discovered, &out, scratch.path()).unwrap();
        assert!(problems.iter().any(|problem| problem.contains("no captured trace")), "{problems:?}");
        row.capture_exclusions = serde_json::from_value(serde_json::json!([
            {"operation":"add", "code":"test-exclusion", "reason":"Must be stale."}
        ]))
        .unwrap();
        row.component = Some("fixture.stateless_add".to_string());
        let problems = row_coverage("other", row, &BTreeSet::from([operation]), &out, scratch.path()).unwrap();
        assert!(
            problems.iter().any(|problem| problem.contains("exclusions are stale")),
            "{problems:?}"
        );
        // Whitespace preserves valid native trace/linkage but breaks the exact
        // manifest digest. Nonselected bundles cannot bypass bundle validation.
        let mut trace = read_bytes(producer.join(trace_file()));
        trace.push(b'\n');
        write_bytes(producer.join(trace_file()), trace).unwrap();
        let error = validate_producer_capture(row, "other", &out, scratch.path()).unwrap_err();
        assert!(error.to_string().contains("bundle validation failed"), "{error}");
    }

    #[test]
    fn old_singleton_and_missing_checks_are_rejected() {
        let root = repo_root(std::env::current_dir().expect("current directory"));
        let document: serde_json::Value = serde_json::from_slice(&read_bytes(repo_path(root, "test/goldens/ctsc/matrix.json"))).unwrap();
        let mut missing = document.clone();
        missing["rows"][0].as_object_mut().unwrap().remove("checks");
        assert!(
            serde_json::from_value::<Matrix>(missing)
                .unwrap_err()
                .to_string()
                .contains("missing field `checks`")
        );
        let mut old = document;
        old["rows"][0]["replay"] = serde_json::json!({"mode": "verified"});
        assert!(
            serde_json::from_value::<Matrix>(old)
                .unwrap_err()
                .to_string()
                .contains("unknown field `replay`")
        );
    }

    pub(super) fn async_registry() -> Registry {
        Registry::parse(
            r#"{"operations":[
                {"name":"fetch","module_path":"fixture","fn_name":"fetch","is_setup":false,"is_async":true,"is_method":false,"is_public":true,"return_type":"String","fills":"","params":[],"component":"fixture.async_operation"},
                {"name":"advance","module_path":"fixture","fn_name":"advance","is_setup":false,"is_async":false,"is_method":true,"is_public":true,"return_type":"()","fills":"","params":[],"component":"fixture.async_setup"},
                {"name":"advance","module_path":"fixture","fn_name":"make","is_setup":true,"is_async":true,"is_method":false,"is_public":true,"return_type":"Counter","fills":"","params":[],"component":"fixture.async_setup"},
                {"name":"add","module_path":"fixture","fn_name":"add","is_setup":false,"is_async":false,"is_method":false,"is_public":true,"return_type":"i32","fills":"","params":[],"component":"fixture.sync"}
            ],"types":[]}"#,
        )
        .expect("synthetic registry parses")
    }

    /// Async operation capture is supported; async setup capture remains rejected
    /// unless the setup-backed operation is excluded exactly.
    #[test]
    pub(super) fn capture_async() {
        let registry = async_registry();
        let unlimited = serde_json::json!([]);
        for component in ["fixture.sync", "fixture.async_operation"] {
            let row = synthetic_row(component, [Phase::Discover, Phase::Capture], &unlimited);
            assert_sync("rust", &registry, &[&row]).unwrap();
        }
    }

    #[test]
    fn async_operation() {
        let registry = async_registry();
        let row = synthetic_row("fixture.async_operation", [Phase::Discover, Phase::Capture], &serde_json::json!([]));
        assert_sync("rust", &registry, &[&row]).unwrap();
    }

    #[test]
    fn async_setup() {
        let registry = async_registry();
        let row = synthetic_row("fixture.async_setup", [Phase::Discover, Phase::Capture], &serde_json::json!([]));
        assert!(
            assert_sync("rust", &registry, &[&row])
                .unwrap_err()
                .to_string()
                .contains("setup 'make'")
        );
    }

    /// The limitation is a claim about the row, not a comment: declaring it
    /// while still capturing would document a restriction the row violates.
    fn async_matrix(phases: impl AsRef<[Phase]>) -> Matrix {
        let limitation = serde_json::json!([{ "code": ASYNC_LIMITATION, "detail": "Capture context is not task-safe." }]);
        Matrix {
            format: MATRIX_FORMAT.to_string(),
            format_version: MATRIX_VERSION.to_string(),
            artifact_root: RepoPath::try_from("test/goldens/ctsc").unwrap(),
            binding: RepoPath::try_from("test/bindings/specgate.yaml").unwrap(),
            sources: SourceCoverage {
                roots: Vec::new(),
                exclusions: Vec::new(),
            },
            rows: vec![synthetic_row("fixture.async_setup", phases, &limitation)],
        }
    }

    #[test]
    pub(super) fn async_limitation() {
        check_shape(&async_matrix([Phase::Discover]));
    }

    #[test]
    #[should_panic(expected = "must stay discovery-only")]
    pub(super) fn rejects_async_capture() {
        check_shape(&async_matrix([Phase::Discover, Phase::Capture]));
    }

    #[test]
    pub(super) fn normalized_errors() {
        let bytes = error_json(ErrorInput {
            id: "negative/private-operation",
            phase: Phase::Discover,
            category: ErrorCategory::Private,
            component: "fixture.private_operation",
            detail: &serde_json::json!({ "message": "operation 'fixture.private_operation::secret' is not public" }),
        });
        let text = String::from_utf8(bytes).expect("utf-8 error document");
        assert!(!text.contains('\n'), "normalized errors must be compact");
        assert!(
            !text.contains(":\\") && !text.contains("file://"),
            "normalized errors must not carry paths"
        );
        let document: serde_json::Value = serde_json::from_str(&text).expect("valid error document");
        assert_eq!(document["format"], "specgate.ctsc-golden-error");
        assert_eq!(document["phase"], "discover");
        assert_eq!(document["outcome"], "failure");
        assert_eq!(document["category"], "private-operation");
        assert_eq!(document["component"], "fixture.private_operation");
    }
}
