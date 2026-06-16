use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::backend::{Backend, GeneratedArtifact};
use crate::mock_backend::MockBackend;
use specgate_types::{
    BindingFile, CaseResult, CaseStatus, RunError, RunOutcome, RunReport, SpecDocument,
};

#[derive(Clone)]
pub struct Harness {
    repo_root: PathBuf,
    backends: HashMap<String, Arc<dyn Backend>>,
}

impl std::fmt::Debug for Harness {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Harness")
            .field("repo_root", &self.repo_root)
            .field("backend_count", &self.backends.len())
            .finish()
    }
}

impl Harness {
    #[must_use]
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        let mut backends: HashMap<String, Arc<dyn Backend>> = HashMap::new();
        backends.insert("mock".to_string(), Arc::new(MockBackend));

        Self {
            repo_root: repo_root.into(),
            backends,
        }
    }

    #[must_use]
    pub fn repo_root(&self) -> &Path {
        &self.repo_root
    }

    pub fn register_backend(&mut self, name: String, backend: Arc<dyn Backend>) {
        self.backends.insert(name, backend);
    }

    #[must_use]
    pub fn backend_names(&self) -> HashSet<String> {
        self.backends.keys().cloned().collect()
    }

    pub fn run_spec(&self, spec_path: impl AsRef<Path>) -> RunOutcome {
        let requested_path = normalize_relative_path(spec_path.as_ref());
        let started_at = Instant::now();

        let result = (|| {
            let spec_path = self.resolve_spec_path(spec_path.as_ref())?;
            let spec = parse_spec_document(&spec_path)?;
            let binding_name = spec.binding_name().unwrap_or_else(|| "mock".to_string());
            let binding = self.resolve_binding(&binding_name)?;
            let backend =
                self.backends
                    .get(&binding.language)
                    .ok_or_else(|| RunError::BackendNotFound {
                        language: binding.language.clone(),
                    })?;

            let workdir = self.prepare_workdir(&spec.name)?;
            let discovery = backend.build_and_discover(&binding, &spec)?;
            let generated = backend.generate(&binding, &spec, &discovery, &workdir)?;
            let run_result = backend.run_command(&binding, &generated);
            let results = backend.collect_results(&generated);
            cleanup_generated_artifacts(&generated, &workdir);
            run_result?;
            let mut results = results?;
            apply_postconditions(&binding, &spec, &generated, &workdir, &mut results);

            Ok(build_report(
                spec.name,
                binding_name,
                results,
                started_at.elapsed().as_millis() as i64,
            ))
        })();

        match result {
            Ok(report) => RunOutcome::Complete { report },
            Err(RunError::SpecNotFound { .. }) => RunOutcome::Error {
                error: RunError::SpecNotFound {
                    path: requested_path,
                },
            },
            Err(error) => RunOutcome::Error { error },
        }
    }

    fn resolve_spec_path(&self, spec_path: &Path) -> Result<PathBuf, RunError> {
        let absolute_path = self.repo_root.join("specs").join(spec_path);

        if absolute_path.is_file() {
            Ok(absolute_path)
        } else {
            Err(RunError::SpecNotFound {
                path: normalize_relative_path(spec_path),
            })
        }
    }

    fn resolve_binding(&self, binding_name: &str) -> Result<BindingFile, RunError> {
        if binding_name == "mock" {
            return Ok(BindingFile {
                language: "mock".to_string(),
                targets: HashMap::new().into_iter().collect(),
            });
        }

        let binding_path = self
            .repo_root
            .join("bindings")
            .join(format!("{binding_name}.yaml"));

        let binding_contents =
            fs::read_to_string(&binding_path).map_err(|_| RunError::BindingNotFound {
                binding: binding_name.to_string(),
            })?;

        let mut binding: BindingFile =
            serde_yaml::from_str(&binding_contents).map_err(|error| RunError::SpecInvalid {
                detail: format!("failed to parse binding {binding_name}: {error}"),
            })?;

        // Resolve relative paths against the binding file's directory
        let binding_dir = binding_path
            .parent()
            .expect("binding path should have a parent directory");
        for target in binding.targets.values_mut() {
            target.package_root = normalize_relative_path(&binding_dir.join(&target.package_root));
            if let Some(test_root) = &target.test_root {
                target.test_root = Some(normalize_relative_path(&binding_dir.join(test_root)));
            }
        }

        Ok(binding)
    }

    fn prepare_workdir(&self, spec_name: &str) -> Result<PathBuf, RunError> {
        let workdir = self
            .repo_root
            .join("rust")
            .join("target")
            .join("specgate-harness")
            .join(spec_name)
            .join(unique_workdir_suffix());

        fs::create_dir_all(&workdir).map_err(|error| RunError::BuildFailed {
            detail: format!("failed to prepare workdir {}: {error}", workdir.display()),
        })?;

        Ok(workdir)
    }
}

fn parse_spec_document(spec_path: &Path) -> Result<SpecDocument, RunError> {
    let spec_contents = fs::read_to_string(spec_path).map_err(|_| RunError::SpecNotFound {
        path: normalize_relative_path(spec_path),
    })?;

    serde_yaml::from_str(&spec_contents).map_err(|error| RunError::SpecInvalid {
        detail: format!("failed to parse spec {}: {error}", spec_path.display()),
    })
}

fn build_report(
    spec_name: String,
    binding: String,
    results: Vec<CaseResult>,
    duration_ms: i64,
) -> RunReport {
    let passed = results
        .iter()
        .filter(|result| result.status == CaseStatus::Pass)
        .count();
    let failed = results.len().saturating_sub(passed);
    let total = results.len();

    RunReport {
        spec_name,
        binding,
        timestamp: current_timestamp(),
        duration_ms,
        results,
        passed,
        failed,
        total,
    }
}

fn current_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("Rfc3339 formatting uses a built-in format description and cannot fail")
}

fn normalize_relative_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn cleanup_generated_artifacts(generated: &GeneratedArtifact, _workdir: &Path) {
    let _ = fs::remove_file(&generated.generated_test_path);
}

fn apply_postconditions(
    binding: &BindingFile,
    spec: &SpecDocument,
    generated: &GeneratedArtifact,
    workdir: &Path,
    results: &mut [CaseResult],
) {
    let cases_by_name = spec
        .cases
        .iter()
        .map(|case| (case.name.as_str(), case))
        .collect::<HashMap<_, _>>();

    for result in results
        .iter_mut()
        .filter(|result| result.status == CaseStatus::Pass)
    {
        let Some(case) = cases_by_name.get(result.name.as_str()) else {
            continue;
        };
        let Some(postconditions) = case.postconditions.as_ref() else {
            continue;
        };

        let passed = postconditions.iter().all(|postcondition| {
            let substituted_inputs: std::collections::BTreeMap<String, String> = postcondition
                .inputs
                .iter()
                .map(|(key, value)| {
                    (
                        key.clone(),
                        substitute_template_vars(value, &generated.generated_test_path, workdir),
                    )
                })
                .collect();
            execute_postcondition_target(&postcondition.target, binding, &substituted_inputs)
        });

        if !passed {
            result.status = CaseStatus::Fail;
        }
    }
}

fn execute_postcondition_target(
    target_name: &str,
    binding: &BindingFile,
    inputs: &std::collections::BTreeMap<String, String>,
) -> bool {
    let Some(target) = binding.targets.get(target_name) else {
        return false;
    };
    let Some(command_template) = &target.command else {
        return false;
    };
    let mut command = command_template.clone();
    for (key, value) in inputs {
        command = command.replace(&format!("{{{key}}}"), value);
    }
    #[cfg(windows)]
    let output = std::process::Command::new("cmd").args(["/C", &command]).output();
    #[cfg(not(windows))]
    let output = std::process::Command::new("sh").args(["-c", &command]).output();
    output.map(|o| o.status.success()).unwrap_or(false)
}

fn substitute_template_vars(template: &str, generated_test_path: &Path, workdir: &Path) -> String {
    template
        .replace(
            "{generated_test_path}",
            generated_test_path.to_string_lossy().as_ref(),
        )
        .replace("{workdir}", workdir.to_string_lossy().as_ref())
}

fn unique_workdir_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after unix epoch")
        .as_nanos();
    format!("run-{nanos}")
}

#[cfg(test)]
mod tests {
    use super::{
        Harness, apply_postconditions, build_report, execute_postcondition_target,
        normalize_relative_path, parse_spec_document, substitute_template_vars, unique_workdir_suffix,
    };
    use crate::backend::{Backend, Discovery, GeneratedArtifact};
    use specgate_types::{
        BindingFile, BindingTarget, CaseResult, CaseStatus, Postcondition, RunError, RunOutcome,
        SpecDocument,
    };
    use std::collections::{BTreeMap, HashMap};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn normalize_relative_path_uses_forward_slashes() {
        assert_eq!(
            normalize_relative_path(Path::new(r"fixtures\simple_fail.spec.yaml")),
            "fixtures/simple_fail.spec.yaml"
        );
    }

    #[test]
    fn build_report_counts_passed_and_failed_cases() {
        let report = build_report(
            "fixture".to_string(),
            "mock".to_string(),
            vec![
                CaseResult {
                    name: "first".to_string(),
                    status: CaseStatus::Pass,
                    duration_ms: 1,
                    traces_file: None,
                    traces_match: None,
                },
                CaseResult {
                    name: "second".to_string(),
                    status: CaseStatus::Fail,
                    duration_ms: 1,
                    traces_file: None,
                    traces_match: None,
                },
            ],
            5,
        );

        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 1);
        assert_eq!(report.total, 2);
        assert_eq!(report.binding, "mock");
        assert_eq!(report.spec_name, "fixture");
        assert_eq!(report.duration_ms, 5);
        assert!(!report.timestamp.is_empty());
    }

    #[test]
    fn harness_debug_includes_repo_root_and_backend_count() {
        let repo_root = scratch_path("harness_debug");
        let harness = Harness::new(&repo_root);
        let debug = format!("{harness:?}");

        assert!(debug.contains("Harness"));
        assert!(debug.contains("backend_count"));
        assert!(debug.contains("harness_debug"));
    }

    #[test]
    fn repo_root_returns_constructor_value() {
        let repo_root = scratch_path("repo_root_getter");
        let harness = Harness::new(&repo_root);

        assert_eq!(harness.repo_root(), repo_root.as_path());
    }

    #[test]
    fn prepare_workdir_returns_build_failed_when_parent_is_a_file() {
        let repo_root = scratch_path("prepare_workdir_error");
        let file_path = repo_root
            .join("rust")
            .join("target")
            .join("specgate-harness");
        fs::create_dir_all(file_path.parent().expect("parent should exist"))
            .expect("parent directories should be created");
        fs::write(&file_path, "not a directory").expect("sentinel file should be written");

        let harness = Harness::new(&repo_root);
        let error = harness
            .prepare_workdir("blocked")
            .expect_err("prepare_workdir should fail");

        assert!(matches!(
            error,
            specgate_types::RunError::BuildFailed { detail }
                if detail.contains("failed to prepare workdir")
        ));
    }

    #[test]
    fn parse_spec_document_returns_spec_not_found_when_file_cannot_be_read() {
        let missing_path = PathBuf::from("missing.spec.yaml");

        let error = parse_spec_document(&missing_path).expect_err("missing file should error");

        assert!(matches!(
            error,
            specgate_types::RunError::SpecNotFound { path } if path == "missing.spec.yaml"
        ));
    }

    #[test]
    fn unique_workdir_suffix_has_run_prefix() {
        let suffix = unique_workdir_suffix();

        assert!(suffix.starts_with("run-"));
        assert!(suffix[4..].chars().all(|ch| ch.is_ascii_digit()));
    }

    #[test]
    fn substitute_template_vars_replaces_known_variables() {
        let result = substitute_template_vars(
            r#"{generated_test_path} and {workdir}"#,
            Path::new(r"generated\specgate_generated.rs"),
            Path::new(r"workdir\run-1"),
        );

        assert!(result.contains(r#"generated\specgate_generated.rs"#));
        assert!(result.contains(r#"workdir\run-1"#));
        assert!(!result.contains("{generated_test_path}"));
        assert!(!result.contains("{workdir}"));
    }

    #[test]
    fn run_spec_returns_build_failed_when_workdir_cannot_be_prepared() {
        let repo_root = scratch_path("run_spec_workdir_error");
        write_spec(&repo_root, "workdir_blocked");
        let blocking_path = repo_root
            .join("rust")
            .join("target")
            .join("specgate-harness")
            .join("workdir_blocked");
        fs::create_dir_all(blocking_path.parent().expect("parent should exist"))
            .expect("parent directories should be created");
        fs::write(&blocking_path, "not a directory").expect("blocking file should be written");

        let outcome = Harness::new(&repo_root).run_spec(Path::new("workdir_blocked.spec.yaml"));

        assert!(matches!(
            outcome,
            RunOutcome::Error {
                error: RunError::BuildFailed { detail }
            } if detail.contains("failed to prepare workdir")
        ));
    }

    #[test]
    fn run_spec_returns_discovery_error() {
        let repo_root = scratch_path("run_spec_discovery_error");
        write_spec(&repo_root, "discovery_error");
        let harness = harness_with_backend(&repo_root, Stage::Discover);

        let outcome = harness.run_spec(Path::new("discovery_error.spec.yaml"));

        assert!(matches!(
            outcome,
            RunOutcome::Error {
                error: RunError::GenerateFailed { detail }
            } if detail == "discovery failed"
        ));
    }

    #[test]
    fn run_spec_returns_collect_results_error() {
        let repo_root = scratch_path("run_spec_collect_error");
        write_spec(&repo_root, "collect_error");
        let harness = harness_with_backend(&repo_root, Stage::Collect);

        let outcome = harness.run_spec(Path::new("collect_error.spec.yaml"));

        assert!(matches!(
            outcome,
            RunOutcome::Error {
                error: RunError::BuildFailed { detail }
            } if detail == "collect failed"
        ));
    }

    #[test]
    fn apply_postconditions_marks_passing_case_failed_when_target_fails() {
        let repo_root = scratch_path("postcondition_failure");
        fs::create_dir_all(&repo_root).expect("repo root should be created");
        // Create a file that we will assert is absent — assertion fails because file exists
        let existing_file = repo_root.join("present.txt");
        fs::write(&existing_file, "still here").expect("existing file should be written");
        let mut results = vec![CaseResult {
            name: "basic_case".to_string(),
            status: CaseStatus::Pass,
            duration_ms: 1,
            traces_file: None,
            traces_match: None,
        }];
        let spec = SpecDocument {
            name: "postcondition_failure".to_string(),
            binding: None,
            depends_on: Vec::new(),
            state: BTreeMap::new(),
            init: BTreeMap::new(),
            operations: BTreeMap::new(),
            invariants: BTreeMap::new(),
            inputs: BTreeMap::new(),
            types: BTreeMap::new(),
            outcome: serde_yaml::Value::String("Complete".to_string()),
            outputs: BTreeMap::new(),
            cases: vec![spec_case_with_postconditions(
                "basic_case",
                vec![Postcondition {
                    target: "assert-file-absent".to_string(),
                    inputs: BTreeMap::from([(
                        "path".to_string(),
                        existing_file.to_string_lossy().to_string(),
                    )]),
                    desc: None,
                }],
            )],
        };
        let generated = GeneratedArtifact {
            generated_test_path: repo_root.join("specgate_generated.rs"),
            results_path: repo_root.join("results.json"),
            cases: Vec::new(),
            spec_name: spec.name.clone(),
        };
        let binding = file_absent_binding();

        apply_postconditions(&binding, &spec, &generated, &repo_root.join("workdir"), &mut results);

        assert_eq!(results[0].status, CaseStatus::Fail);
    }

    #[test]
    fn execute_postcondition_target_passes_when_file_absent() {
        let binding = file_absent_binding();
        let absent_path = scratch_path("absent_file_check").join("definitely_absent.txt");
        let inputs = BTreeMap::from([("path".to_string(), absent_path.to_string_lossy().to_string())]);

        assert!(execute_postcondition_target("assert-file-absent", &binding, &inputs));
    }

    #[test]
    fn execute_postcondition_target_returns_false_for_unknown_target() {
        let binding = BindingFile {
            language: "mock".to_string(),
            targets: BTreeMap::new(),
        };
        let inputs = BTreeMap::new();

        assert!(!execute_postcondition_target("assert-file-absent", &binding, &inputs));
    }

    #[test]
    fn execute_postcondition_target_returns_false_for_target_without_command() {
        let binding = BindingFile {
            language: "mock".to_string(),
            targets: BTreeMap::from([(
                "assert-file-absent".to_string(),
                BindingTarget {
                    package_root: ".".to_string(),
                    test_root: None,
                    build: None,
                    command: None,
                    function: None,
                    constructor: None,
                    outputs: Default::default(),
                },
            )]),
        };
        let inputs = BTreeMap::new();

        assert!(!execute_postcondition_target("assert-file-absent", &binding, &inputs));
    }

    /// Returns a `BindingFile` with an `assert-file-absent` target whose command
    /// exits 1 if `{path}` exists and 0 if it is absent, using the shell that the
    /// production `execute_postcondition_target` actually invokes on this platform.
    fn file_absent_binding() -> BindingFile {
        #[cfg(windows)]
        let command = "IF EXIST {path} exit 1".to_string();
        #[cfg(not(windows))]
        let command = r#"[ ! -e '{path}' ]"#.to_string();

        BindingFile {
            language: "mock".to_string(),
            targets: BTreeMap::from([(
                "assert-file-absent".to_string(),
                BindingTarget {
                    package_root: ".".to_string(),
                    test_root: None,
                    build: None,
                    command: Some(command),
                    function: None,
                    constructor: None,
                    outputs: Default::default(),
                },
            )]),
        }
    }

    fn scratch_path(test_name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-scratch")
            .join(format!("{test_name}-{}", unique_suffix()))
    }

    fn write_spec(repo_root: &Path, spec_name: &str) {
        let spec_path = repo_root
            .join("specs")
            .join(format!("{spec_name}.spec.yaml"));
        fs::create_dir_all(spec_path.parent().expect("spec parent should exist"))
            .expect("spec directory should be created");
        fs::write(
            spec_path,
            format!(
                "name: {spec_name}\ntarget: test\noutcome: Complete\noutputs:\n  when Complete:\n    report: RunReport\ncases: []\n"
            ),
        )
        .expect("spec fixture should be written");
    }

    fn spec_case_with_postconditions(name: &str, postconditions: Vec<Postcondition>) -> specgate_types::SpecCase {
        specgate_types::SpecCase {
            name: name.to_string(),
            desc: format!("case {name}"),
            binding: None,
            inputs: BTreeMap::new(),
            expected: BTreeMap::new(),
            steps: Vec::new(),
            postconditions: Some(postconditions),
        }
    }

    fn harness_with_backend(repo_root: &Path, stage: Stage) -> Harness {
        let mut backends: HashMap<String, Arc<dyn Backend>> = HashMap::new();
        backends.insert("mock".to_string(), Arc::new(FailingBackend { stage }));

        Harness {
            repo_root: repo_root.to_path_buf(),
            backends,
        }
    }

    #[derive(Clone, Copy)]
    enum Stage {
        Discover,
        Collect,
    }

    struct FailingBackend {
        stage: Stage,
    }

    impl Backend for FailingBackend {
        fn build_and_discover(
            &self,
            _binding: &BindingFile,
            _spec: &SpecDocument,
        ) -> Result<Discovery, RunError> {
            match self.stage {
                Stage::Discover => Err(RunError::GenerateFailed {
                    detail: "discovery failed".to_string(),
                }),
                Stage::Collect => Ok(Discovery { cases: Vec::new() }),
            }
        }

        fn generate(
            &self,
            _binding: &BindingFile,
            spec: &SpecDocument,
            discovery: &Discovery,
            workdir: &Path,
        ) -> Result<GeneratedArtifact, RunError> {
            Ok(GeneratedArtifact {
                generated_test_path: workdir.join("generated.mock"),
                results_path: workdir.join("results.json"),
                cases: discovery.cases.clone(),
                spec_name: spec.name.clone(),
            })
        }

        fn run_command(
            &self,
            _binding: &BindingFile,
            _generated: &GeneratedArtifact,
        ) -> Result<(), RunError> {
            Ok(())
        }

        fn collect_results(
            &self,
            _generated: &GeneratedArtifact,
        ) -> Result<Vec<CaseResult>, RunError> {
            Err(RunError::BuildFailed {
                detail: "collect failed".to_string(),
            })
        }
    }

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos()
    }
}
