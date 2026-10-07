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
        assert!(mode.contains("verified") && mode.contains("not-applicable"), "{mode}");
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
            ),
            "duplicate-operation-identity"
        );
        assert_eq!(
            error_category(
                "setup 'make_counter' for 'fixture.missing_operation::increment' has no operation to construct; annotate the operation or remove the setup"
            ),
            "orphan-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.missing_setup::increment' is a method with no receiver setup; annotate a #[spec_setup(\"increment\")] producer for its receiver"
            ),
            "method-missing-setup"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.private_operation::secret' is declared on private function 'secret'; discovery exposes only public operations"
            ),
            "private-operation"
        );
        assert_eq!(
            error_category(
                "operation 'fixture.value::echo' output type 'value' is the dynamic runtime value; CTSC registries require declared semantic types"
            ),
            "dynamic-value-type"
        );
        assert_eq!(
            error_category("referenced type 'Gadget' has no registered component owner"),
            "unresolved-type"
        );
        assert_eq!(error_category("something else entirely"), "unclassified");
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
        let artifacts = repo_path(&root, &matrix.artifact_root);
        let exceptions = matrix
            .rows
            .iter()
            .filter(|row| row.parity.mode == ParityMode::Exception)
            .collect::<Vec<_>>();
        assert!(!exceptions.is_empty(), "the matrix still declares parity exceptions");
        for row in exceptions {
            let rust: serde_json::Value =
                serde_json::from_slice(&read_bytes(row.rust_dir(&artifacts).join(registry_file()))).expect("Rust registry JSON");
            let csharp: serde_json::Value =
                serde_json::from_slice(&read_bytes(row.csharp_dir(&artifacts).join(registry_file()))).expect("C# registry JSON");
            assert_eq!(
                semantic_differences(&rust, &csharp),
                row.parity.allowed_differences,
                "{}: the declared parity exception must be the exact observed difference set",
                row.id
            );
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
        let artifacts = if capturing {
            serde_json::json!([manifest_file(), trace_file(), registry_file()])
        } else {
            serde_json::json!([registry_file()])
        };
        let replay = if capturing {
            serde_json::json!({ "mode": "verified" })
        } else {
            serde_json::json!({ "mode": "not-applicable", "reason": "Discovery-only rows own no bundle." })
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
                "artifacts": artifacts,
            },
            "parity": { "mode": "rust-only", "reason": "Synthetic row used only by this test." },
            "replay": replay,
            "linkage": linkage,
            "limitations": limitations,
        }))
        .expect("synthetic row deserializes")
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

    /// Capture cannot instrument async work in either direction: an async
    /// operation rejects capture from its own body and an async setup records
    /// nothing at all. A row that claims a capture bundle for such a component
    /// must fail here, naming the async declaration.
    #[test]
    pub(super) fn capture_async() {
        let registry = async_registry();
        let unlimited = serde_json::json!([]);
        let synchronous = synthetic_row("fixture.sync", [Phase::Discover, Phase::Capture], &unlimited);
        assert_sync("rust", &registry, &[&synchronous]).unwrap();

        let limitation = serde_json::json!([{ "code": ASYNC_LIMITATION, "detail": "Capture context is not task-safe." }]);
        for component in ["fixture.async_operation", "fixture.async_setup"] {
            let discovery_only = synthetic_row(component, [Phase::Discover], &limitation);
            assert_sync("rust", &registry, &[&discovery_only]).unwrap();

            let mut excluded = synthetic_row(component, [Phase::Discover, Phase::Capture], &unlimited);
            excluded.capture_exclusions.push(CaptureExclusion {
                operation: if component == "fixture.async_operation" {
                    "fetch".to_string()
                } else {
                    "advance".to_string()
                },
                code: ASYNC_LIMITATION.to_string(),
                reason: "Native capture context is not task-safe.".to_string(),
            });
            assert_sync("rust", &registry, &[&excluded]).unwrap();
        }
    }

    #[test]
    fn async_operation() {
        let registry = async_registry();
        let row = synthetic_row("fixture.async_operation", [Phase::Discover, Phase::Capture], &serde_json::json!([]));
        assert!(
            assert_sync("rust", &registry, &[&row])
                .unwrap_err()
                .to_string()
                .contains("operation 'fetch'")
        );
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
            bindings: BTreeMap::from([("rust".to_string(), RepoPath::try_from("test/bindings/rust.yaml").unwrap())]),
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
            category: "private-operation",
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
