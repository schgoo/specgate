use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use specgate_harness::{Backend, DiscoveredCase, Discovery, GeneratedArtifact};
use specgate_types::{BindingFile, CaseResult, RunError, SpecDocument};

use crate::annotations::Annotation;
use crate::generator::generate_test_file;

#[derive(Debug, Default)]
pub struct RustBackend {
    annotations_by_project: Mutex<BTreeMap<PathBuf, Vec<Annotation>>>,
}

impl Backend for RustBackend {
    fn build_and_discover(
        &self,
        binding: &BindingFile,
        spec: &SpecDocument,
    ) -> Result<Discovery, RunError> {
        let project_root = project_root_path(binding);
        let registry_path = annotation_registry_path(&project_root);

        let annotations = if registry_path.is_file() {
            load_annotations(&registry_path)?
        } else if project_root.join("Cargo.toml").is_file() {
            run_build(&project_root)?;
            if registry_path.is_file() {
                load_annotations(&registry_path)?
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        self.annotations_by_project
            .lock()
            .expect("annotation cache mutex should not be poisoned")
            .insert(project_root, annotations);

        Ok(Discovery {
            cases: spec
                .cases
                .iter()
                .map(|case| DiscoveredCase {
                    raw_name: case.name.clone(),
                    mock_status: None,
                })
                .collect(),
        })
    }

    fn generate(
        &self,
        binding: &BindingFile,
        spec: &SpecDocument,
        discovery: &Discovery,
        workdir: &Path,
    ) -> Result<GeneratedArtifact, RunError> {
        let project_root = project_root_path(binding);
        let generated_test_path = project_root.join("tests").join("specgate_generated.rs");
        let results_path = workdir.join("results.json");
        let annotations = self.annotations_for_project(&project_root)?;

        let file = generate_test_file(
            spec,
            &annotations,
            binding.targets.get(&spec.target),
            &generated_test_path,
            &results_path,
        )
        .map_err(|errors| RunError::GenerateFailed {
            detail: format!(
                "failed to generate {}: {}",
                spec.name,
                serde_json::to_string(&errors).expect("GenerateError serialization should succeed")
            ),
        })?;

        let parent = file
            .path
            .parent()
            .expect("generated Rust test path should always have a parent directory");
        fs::create_dir_all(parent).map_err(|error| RunError::GenerateFailed {
            detail: format!("failed to create {}: {error}", parent.display()),
        })?;
        fs::write(&file.path, &file.content).map_err(|error| RunError::GenerateFailed {
            detail: format!("failed to write {}: {error}", file.path.display()),
        })?;

        Ok(GeneratedArtifact {
            generated_test_path,
            results_path,
            cases: discovery.cases.clone(),
            spec_name: spec.name.clone(),
        })
    }

    fn run_command(
        &self,
        binding: &BindingFile,
        generated: &GeneratedArtifact,
    ) -> Result<(), RunError> {
        let project_root = project_root_path(binding);
        if !project_root.join("Cargo.toml").is_file() {
            return Err(RunError::BuildFailed {
                detail: format!("missing Cargo.toml under {}", project_root.display()),
            });
        }

        let status = run_test_command("cargo", &project_root, &generated.results_path)?;

        if status.success() {
            Ok(())
        } else {
            Err(RunError::BuildFailed {
                detail: format!(
                    "cargo test --test specgate_generated failed in {} with status {status}",
                    project_root.display()
                ),
            })
        }
    }

    fn collect_results(&self, generated: &GeneratedArtifact) -> Result<Vec<CaseResult>, RunError> {
        let results =
            fs::read_to_string(&generated.results_path).map_err(|error| RunError::BuildFailed {
                detail: format!(
                    "failed to read results for {}: {error}",
                    generated.spec_name
                ),
            })?;

        results
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                serde_json::from_str::<CaseResult>(line).map_err(|error| RunError::BuildFailed {
                    detail: format!(
                        "failed to parse result line for {}: {error}",
                        generated.spec_name
                    ),
                })
            })
            .collect()
    }
}

fn run_test_command(
    cargo_program: &str,
    project_root: &Path,
    results_path: &Path,
) -> Result<std::process::ExitStatus, RunError> {
    Command::new(cargo_program)
        .current_dir(project_root)
        .arg("test")
        .arg("--test")
        .arg("specgate_generated")
        .arg("--features")
        .arg("specgate")
        .arg("--")
        .arg("--test-threads=1")
        .arg("--nocapture")
        .env("SPECGATE_RESULTS_PATH", results_path)
        .status()
        .map_err(|error| RunError::BuildFailed {
            detail: format!(
                "failed to run cargo test in {}: {error}",
                project_root.display()
            ),
        })
}

impl RustBackend {
    fn annotations_for_project(&self, project_root: &Path) -> Result<Vec<Annotation>, RunError> {
        if let Some(annotations) = self
            .annotations_by_project
            .lock()
            .expect("annotation cache mutex should not be poisoned")
            .get(project_root)
            .cloned()
        {
            return Ok(annotations);
        }

        let registry_path = annotation_registry_path(project_root);
        if registry_path.is_file() {
            load_annotations(&registry_path)
        } else {
            Ok(Vec::new())
        }
    }
}

fn project_root_path(binding: &BindingFile) -> PathBuf {
    PathBuf::from(&binding.project_root)
}

fn annotation_registry_path(project_root: &Path) -> PathBuf {
    project_root
        .join("target")
        .join("specgate")
        .join("annotations.json")
}

fn load_annotations(path: &Path) -> Result<Vec<Annotation>, RunError> {
    let contents = fs::read_to_string(path).map_err(|error| RunError::BuildFailed {
        detail: format!(
            "failed to read annotation registry {}: {error}",
            path.display()
        ),
    })?;
    serde_json::from_str(&contents).map_err(|error| RunError::BuildFailed {
        detail: format!(
            "failed to parse annotation registry {}: {error}",
            path.display()
        ),
    })
}

fn run_build(project_root: &Path) -> Result<(), RunError> {
    let status = run_build_with_program("cargo", project_root)?;

    if status.success() {
        Ok(())
    } else {
        Err(RunError::BuildFailed {
            detail: format!(
                "cargo test --no-run --features specgate failed in {} with status {status}",
                project_root.display()
            ),
        })
    }
}

fn run_build_with_program(
    cargo_program: &str,
    project_root: &Path,
) -> Result<std::process::ExitStatus, RunError> {
    Command::new(cargo_program)
        .current_dir(project_root)
        .arg("test")
        .arg("--no-run")
        .arg("--features")
        .arg("specgate")
        .status()
        .map_err(|error| RunError::BuildFailed {
            detail: format!(
                "failed to build project {}: {error}",
                project_root.display()
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::{
        RustBackend, annotation_registry_path, load_annotations, run_build_with_program,
        run_test_command,
    };
    use crate::annotations::{Annotation, OperationKind};
    use specgate_harness::{Backend, GeneratedArtifact};
    use specgate_types::{BindingFile, CaseResult, CaseStatus, SpecCase, SpecDocument};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn build_and_discover_reads_registry_and_cases() {
        let project_root = create_project_root("build_and_discover_reads_registry");
        write_annotation_registry(
            &project_root,
            &[Annotation::SpecOperation {
                operation: "calc".to_string(),
                kind: OperationKind::Stateless,
                symbol: "calc::add".to_string(),
            }],
        );
        let backend = RustBackend::default();
        let discovery = backend
            .build_and_discover(&binding(&project_root), &spec("calc"))
            .expect("discovery should succeed");

        assert_eq!(discovery.cases.len(), 1);
        assert_eq!(discovery.cases[0].raw_name, "basic");
    }

    #[test]
    fn build_and_discover_returns_error_for_invalid_registry() {
        let project_root = create_project_root("build_and_discover_invalid_registry");
        let registry_path = annotation_registry_path(&project_root);
        fs::create_dir_all(
            registry_path
                .parent()
                .expect("registry parent should exist"),
        )
        .expect("registry parent should be created");
        fs::write(&registry_path, "not json").expect("invalid registry should be written");

        let backend = RustBackend::default();
        let error = backend
            .build_and_discover(&binding(&project_root), &spec("calc"))
            .expect_err("discovery should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to parse annotation registry")
        ));
    }

    #[test]
    fn build_and_discover_runs_build_when_registry_is_missing() {
        let project_root = create_project_root("build_and_discover_runs_build");
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"fixture_build\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n\n[features]\nspecgate = []\n\n[workspace]\n",
        )
        .expect("cargo manifest should be written");
        fs::create_dir_all(project_root.join("src")).expect("src dir should exist");
        fs::write(project_root.join("src").join("lib.rs"), "").expect("lib should be written");
        fs::write(
            project_root.join("build.rs"),
            "use std::env;\nuse std::fs;\nuse std::path::PathBuf;\n\nfn main() {\n    let out_dir = PathBuf::from(env::var(\"CARGO_MANIFEST_DIR\").expect(\"manifest dir\"));\n    let registry = out_dir.join(\"target\").join(\"specgate\").join(\"annotations.json\");\n    fs::create_dir_all(registry.parent().expect(\"registry parent\")).expect(\"create registry parent\");\n    fs::write(registry, r#\"[{\"SpecOperation\":{\"operation\":\"calc\",\"kind\":\"Stateless\",\"symbol\":\"calc::add\"}}]\"#).expect(\"write registry\");\n}\n",
        )
        .expect("build script should be written");

        let backend = RustBackend::default();
        let discovery = backend
            .build_and_discover(&binding(&project_root), &spec("calc"))
            .expect("discovery should succeed");

        assert_eq!(discovery.cases.len(), 1);
        assert!(annotation_registry_path(&project_root).is_file());
    }

    #[test]
    fn build_and_discover_runs_build_and_keeps_empty_registry_when_none_is_emitted() {
        let project_root = create_project_root("build_and_discover_runs_build_without_registry");
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"fixture_build_empty\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nspecgate = []\n\n[workspace]\n",
        )
        .expect("cargo manifest should be written");
        fs::create_dir_all(project_root.join("src")).expect("src dir should exist");
        fs::write(project_root.join("src").join("lib.rs"), "").expect("lib should be written");

        let backend = RustBackend::default();
        let discovery = backend
            .build_and_discover(&binding(&project_root), &spec("calc"))
            .expect("discovery should succeed");

        assert_eq!(discovery.cases.len(), 1);
        assert!(!annotation_registry_path(&project_root).is_file());
    }

    #[test]
    fn build_and_discover_without_manifest_or_registry_uses_empty_annotations() {
        let project_root = create_project_root("build_and_discover_without_manifest");
        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let spec = spec("calc");
        let discovery = backend
            .build_and_discover(&binding, &spec)
            .expect("discovery should succeed");
        let error = backend
            .generate(
                &binding,
                &spec,
                &discovery,
                &scratch_dir("build_and_discover_without_manifest_workdir"),
            )
            .expect_err("generation should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::GenerateFailed { detail }
                if detail.contains("missing SpecOperation annotation")
        ));
    }

    #[test]
    fn generate_writes_generated_test_into_project_tests_dir() {
        let project_root = create_project_root("generate_writes_generated_test");
        write_annotation_registry(
            &project_root,
            &[
                Annotation::SpecOperation {
                    operation: "calc".to_string(),
                    kind: OperationKind::Stateless,
                    symbol: "calc::Calculator::add".to_string(),
                },
                Annotation::SpecSetup {
                    operation: "calc".to_string(),
                    name: "default".to_string(),
                    symbol: "calc::setup_calc".to_string(),
                    params: Vec::new(),
                    returns: None,
                },
                Annotation::SpecCapture {
                    operation: "calc".to_string(),
                    symbol: "calc::Calculator::result".to_string(),
                    capture_all: false,
                },
            ],
        );
        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let spec = spec("calc");
        let discovery = backend
            .build_and_discover(&binding, &spec)
            .expect("discovery should succeed");
        let workdir = scratch_dir("generate_writes_generated_test_workdir");

        let generated = backend
            .generate(&binding, &spec, &discovery, &workdir)
            .expect("generation should succeed");

        assert_eq!(
            generated.generated_test_path,
            project_root.join("tests").join("specgate_generated.rs")
        );
        let contents = fs::read_to_string(&generated.generated_test_path)
            .expect("generated test file should be readable");
        assert!(contents.contains("calc::setup_calc"));
        assert_eq!(generated.results_path, workdir.join("results.json"));
    }

    #[test]
    fn generate_returns_generate_failed_for_missing_setup() {
        let project_root = create_project_root("generate_missing_setup");
        write_annotation_registry(
            &project_root,
            &[Annotation::SpecOperation {
                operation: "calc".to_string(),
                kind: OperationKind::Stateless,
                symbol: "calc::add".to_string(),
            }],
        );
        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let spec = spec("calc");
        let discovery = backend
            .build_and_discover(&binding, &spec)
            .expect("discovery should succeed");

        let error = backend
            .generate(
                &binding,
                &spec,
                &discovery,
                &scratch_dir("generate_missing_setup_workdir"),
            )
            .expect_err("generation should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::GenerateFailed { detail }
                if detail.contains("MissingSetup")
        ));
    }

    #[test]
    fn generate_loads_annotations_without_cached_discovery() {
        let project_root = create_project_root("generate_loads_annotations_without_cache");
        write_annotation_registry(
            &project_root,
            &[
                Annotation::SpecOperation {
                    operation: "calc".to_string(),
                    kind: OperationKind::Stateless,
                    symbol: "calc::add".to_string(),
                },
                Annotation::SpecSetup {
                    operation: "calc".to_string(),
                    name: "default".to_string(),
                    symbol: "calc::setup".to_string(),
                    params: Vec::new(),
                    returns: None,
                },
            ],
        );

        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let generated = backend
            .generate(
                &binding,
                &spec("calc"),
                &specgate_harness::Discovery {
                    cases: vec![specgate_harness::DiscoveredCase {
                        raw_name: "basic".to_string(),
                        mock_status: None,
                    }],
                },
                &scratch_dir("generate_loads_annotations_without_cache_workdir"),
            )
            .expect("generation should succeed");

        assert!(generated.generated_test_path.is_file());
    }

    #[test]
    fn generate_without_registry_or_cache_uses_empty_annotations() {
        let project_root = create_project_root("generate_without_registry_or_cache");
        let backend = RustBackend::default();
        let error = backend
            .generate(
                &binding(&project_root),
                &spec("calc"),
                &specgate_harness::Discovery {
                    cases: vec![specgate_harness::DiscoveredCase {
                        raw_name: "basic".to_string(),
                        mock_status: None,
                    }],
                },
                &scratch_dir("generate_without_registry_or_cache_workdir"),
            )
            .expect_err("generation should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::GenerateFailed { detail }
                if detail.contains("missing SpecOperation annotation")
        ));
    }

    #[test]
    fn generate_returns_error_when_tests_parent_cannot_be_created() {
        let project_root = scratch_dir("generate_parent_create_error");
        fs::write(project_root.join("tests"), "not a directory")
            .expect("blocking file should be written");
        write_annotation_registry(
            &project_root,
            &[
                Annotation::SpecOperation {
                    operation: "calc".to_string(),
                    kind: OperationKind::Stateless,
                    symbol: "calc::add".to_string(),
                },
                Annotation::SpecSetup {
                    operation: "calc".to_string(),
                    name: "default".to_string(),
                    symbol: "calc::setup".to_string(),
                    params: Vec::new(),
                    returns: None,
                },
            ],
        );

        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let spec = spec("calc");
        let discovery = backend
            .build_and_discover(&binding, &spec)
            .expect("discovery should succeed");
        let error = backend
            .generate(
                &binding,
                &spec,
                &discovery,
                &scratch_dir("generate_parent_create_error_workdir"),
            )
            .expect_err("generation should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::GenerateFailed { detail }
                if detail.contains("failed to create")
        ));
    }

    #[test]
    fn generate_returns_error_when_generated_path_is_a_directory() {
        let project_root = create_project_root("generate_write_error");
        write_annotation_registry(
            &project_root,
            &[
                Annotation::SpecOperation {
                    operation: "calc".to_string(),
                    kind: OperationKind::Stateless,
                    symbol: "calc::add".to_string(),
                },
                Annotation::SpecSetup {
                    operation: "calc".to_string(),
                    name: "default".to_string(),
                    symbol: "calc::setup".to_string(),
                    params: Vec::new(),
                    returns: None,
                },
            ],
        );
        fs::create_dir_all(project_root.join("tests").join("specgate_generated.rs"))
            .expect("generated path directory should exist");

        let backend = RustBackend::default();
        let binding = binding(&project_root);
        let spec = spec("calc");
        let discovery = backend
            .build_and_discover(&binding, &spec)
            .expect("discovery should succeed");
        let error = backend
            .generate(
                &binding,
                &spec,
                &discovery,
                &scratch_dir("generate_write_error_workdir"),
            )
            .expect_err("generation should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::GenerateFailed { detail }
                if detail.contains("failed to write")
        ));
    }

    #[test]
    fn run_command_executes_generated_test() {
        let project_root = create_project_root("run_command_executes_generated_test");
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nspecgate = []\n\n[workspace]\n",
        )
        .expect("cargo manifest should be written");
        fs::create_dir_all(project_root.join("src")).expect("src dir should exist");
        fs::write(project_root.join("src").join("lib.rs"), "").expect("lib should be written");
        fs::create_dir_all(project_root.join("tests")).expect("tests dir should exist");
        fs::write(
            project_root.join("tests").join("specgate_generated.rs"),
            "#[test]\nfn generated_passes() { assert_eq!(2 + 2, 4); }\n",
        )
        .expect("generated test should be written");

        let backend = RustBackend::default();
        backend
            .run_command(
                &binding(&project_root),
                &GeneratedArtifact {
                    generated_test_path: project_root.join("tests").join("specgate_generated.rs"),
                    results_path: project_root.join("results.json"),
                    cases: Vec::new(),
                    spec_name: "calc".to_string(),
                },
            )
            .expect("run command should succeed");
    }

    #[test]
    fn run_command_returns_error_when_generated_test_fails() {
        let project_root = create_project_root("run_command_test_failure");
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"fixture_fail\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\nspecgate = []\n\n[workspace]\n",
        )
        .expect("cargo manifest should be written");
        fs::create_dir_all(project_root.join("src")).expect("src dir should exist");
        fs::write(project_root.join("src").join("lib.rs"), "").expect("lib should be written");
        fs::write(
            project_root.join("tests").join("specgate_generated.rs"),
            "#[test]\nfn generated_fails() { panic!(\"boom\"); }\n",
        )
        .expect("generated test should be written");

        let backend = RustBackend::default();
        let error = backend
            .run_command(
                &binding(&project_root),
                &GeneratedArtifact {
                    generated_test_path: project_root.join("tests").join("specgate_generated.rs"),
                    results_path: project_root.join("results.json"),
                    cases: Vec::new(),
                    spec_name: "calc".to_string(),
                },
            )
            .expect_err("run command should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("cargo test --test specgate_generated failed")
        ));
    }

    #[test]
    fn run_command_returns_error_when_manifest_is_missing() {
        let backend = RustBackend::default();
        let project_root = scratch_dir("run_command_manifest_missing");
        let error = backend
            .run_command(
                &binding(&project_root),
                &GeneratedArtifact {
                    generated_test_path: project_root.join("tests").join("specgate_generated.rs"),
                    results_path: project_root.join("results.json"),
                    cases: Vec::new(),
                    spec_name: "calc".to_string(),
                },
            )
            .expect_err("run command should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("missing Cargo.toml")
        ));
    }

    #[test]
    fn run_test_command_returns_error_when_cargo_cannot_start() {
        let error = run_test_command(
            "definitely-not-a-real-cargo-binary",
            Path::new("."),
            Path::new("results.json"),
        )
        .expect_err("run should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to run cargo test")
        ));
    }

    #[test]
    fn load_annotations_returns_error_when_registry_is_missing() {
        let path = scratch_dir("load_annotations_missing").join("missing.json");
        let error = load_annotations(&path).expect_err("load should fail");
        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to read annotation registry")
        ));
    }

    #[test]
    fn run_build_returns_error_when_cargo_cannot_start() {
        let error = run_build_with_program("definitely-not-a-real-cargo-binary", Path::new("."))
            .expect_err("build should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to build project")
        ));
    }

    #[test]
    fn run_build_returns_error_when_cargo_fails() {
        let project_root = create_project_root("run_build_failure");
        fs::write(
            project_root.join("Cargo.toml"),
            "[package]\nname = \"fixture_build_fail\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
        )
        .expect("cargo manifest should be written");
        fs::create_dir_all(project_root.join("src")).expect("src dir should exist");
        fs::write(project_root.join("src").join("lib.rs"), "").expect("lib should be written");

        let error = super::run_build(&project_root).expect_err("build should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("cargo test --no-run --features specgate failed")
        ));
    }

    #[test]
    fn collect_results_parses_ndjson() {
        let backend = RustBackend::default();
        let workdir = scratch_dir("collect_results_parses_ndjson");
        let generated = GeneratedArtifact {
            generated_test_path: workdir.join("tests").join("specgate_generated.rs"),
            results_path: workdir.join("results.json"),
            cases: Vec::new(),
            spec_name: "calc".to_string(),
        };
        fs::write(
            &generated.results_path,
            "{\"name\":\"alpha\",\"status\":\"pass\",\"duration_ms\":1}\n{\"name\":\"beta\",\"status\":\"fail\",\"duration_ms\":2}\n",
        )
        .expect("results should be written");

        let results = backend
            .collect_results(&generated)
            .expect("results should parse");

        assert_eq!(
            results,
            vec![
                CaseResult {
                    name: "alpha".to_string(),
                    status: CaseStatus::Pass,
                    duration_ms: 1,
                },
                CaseResult {
                    name: "beta".to_string(),
                    status: CaseStatus::Fail,
                    duration_ms: 2,
                },
            ]
        );
    }

    #[test]
    fn collect_results_returns_error_when_results_are_missing() {
        let backend = RustBackend::default();
        let workdir = scratch_dir("collect_results_missing");
        let generated = GeneratedArtifact {
            generated_test_path: workdir.join("tests").join("specgate_generated.rs"),
            results_path: workdir.join("results.json"),
            cases: Vec::new(),
            spec_name: "calc".to_string(),
        };

        let error = backend
            .collect_results(&generated)
            .expect_err("collect should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to read results")
        ));
    }

    #[test]
    fn collect_results_returns_error_for_invalid_line() {
        let backend = RustBackend::default();
        let workdir = scratch_dir("collect_results_invalid_line");
        let generated = GeneratedArtifact {
            generated_test_path: workdir.join("tests").join("specgate_generated.rs"),
            results_path: workdir.join("results.json"),
            cases: Vec::new(),
            spec_name: "calc".to_string(),
        };
        fs::write(&generated.results_path, "not json\n").expect("invalid line should be written");

        let error = backend
            .collect_results(&generated)
            .expect_err("collect should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to parse result line")
        ));
    }

    #[test]
    fn collect_results_ignores_blank_lines() {
        let backend = RustBackend::default();
        let workdir = scratch_dir("collect_results_blank_lines");
        let generated = GeneratedArtifact {
            generated_test_path: workdir.join("tests").join("specgate_generated.rs"),
            results_path: workdir.join("results.json"),
            cases: Vec::new(),
            spec_name: "calc".to_string(),
        };
        fs::write(
            &generated.results_path,
            "\n{\"name\":\"alpha\",\"status\":\"pass\",\"duration_ms\":1}\n\n",
        )
        .expect("results should be written");

        let results = backend
            .collect_results(&generated)
            .expect("results should parse");

        assert_eq!(results.len(), 1);
    }

    fn binding(project_root: &Path) -> BindingFile {
        BindingFile {
            language: "rust".to_string(),
            project_root: project_root.display().to_string(),
            targets: BTreeMap::new(),
        }
    }

    fn spec(name: &str) -> SpecDocument {
        SpecDocument {
            name: name.to_string(),
            binding: Some("rust".to_string()),
            target: "test".to_string(),
            inputs: BTreeMap::new(),
            types: BTreeMap::new(),
            outcome: serde_yaml::Value::String("Ok".to_string()),
            outputs: BTreeMap::new(),
            cases: vec![SpecCase {
                name: "basic".to_string(),
                desc: "basic case".to_string(),
                inputs: BTreeMap::from([
                    (
                        "a".to_string(),
                        serde_yaml::to_value(1).expect("value should serialize"),
                    ),
                    (
                        "b".to_string(),
                        serde_yaml::to_value(2).expect("value should serialize"),
                    ),
                ]),
                expected: BTreeMap::from([
                    (
                        "outcome".to_string(),
                        serde_yaml::Value::String("Ok".to_string()),
                    ),
                    (
                        "result".to_string(),
                        serde_yaml::to_value(3).expect("value should serialize"),
                    ),
                ]),
            }],
        }
    }

    fn write_annotation_registry(project_root: &Path, annotations: &[Annotation]) {
        let registry_path = annotation_registry_path(project_root);
        fs::create_dir_all(
            registry_path
                .parent()
                .expect("registry parent should exist"),
        )
        .expect("registry parent should be created");
        fs::write(
            registry_path,
            serde_json::to_string(annotations).expect("annotations should serialize"),
        )
        .expect("annotation registry should be written");
    }

    fn create_project_root(test_name: &str) -> PathBuf {
        let root = scratch_dir(test_name);
        fs::create_dir_all(root.join("tests")).expect("tests dir should be created");
        root
    }

    fn scratch_dir(test_name: &str) -> PathBuf {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{test_name}-{}", unique_suffix()));
        fs::create_dir_all(&path).expect("scratch dir should be created");
        path
    }

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos()
    }
}
