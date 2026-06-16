use std::fs;
use std::path::Path;

use crate::backend::{Backend, DiscoveredCase, Discovery, GeneratedArtifact};
use specgate_types::{BindingFile, CaseResult, CaseStatus, RunError, SpecDocument};

pub struct MockBackend;

impl Backend for MockBackend {
    fn build_and_discover(
        &self,
        _binding: &BindingFile,
        spec: &SpecDocument,
    ) -> Result<Discovery, RunError> {
        let mut cases = Vec::with_capacity(spec.cases.len());
        for case in &spec.cases {
            let mock_status = match case.inputs.get("mock_result") {
                Some(value) => value.as_str().map(String::from),
                None => None,
            };

            cases.push(DiscoveredCase {
                raw_name: case.name.clone(),
                mock_status,
            });
        }

        Ok(Discovery { cases })
    }

    fn generate(
        &self,
        _binding: &BindingFile,
        spec: &SpecDocument,
        discovery: &Discovery,
        workdir: &Path,
    ) -> Result<GeneratedArtifact, RunError> {
        if spec.name.contains("generate_error") {
            return Err(RunError::GenerateFailed {
                detail: format!("mock backend refused to generate {}", spec.name),
            });
        }

        let generated_test_path = workdir.join("generated_tests.mock");
        let results_path = workdir.join("results.json");

        let mut generated_file = String::new();
        for case in &discovery.cases {
            if !generated_file.is_empty() {
                generated_file.push('\n');
            }
            generated_file.push_str(&case.raw_name);
        }

        fs::write(&generated_test_path, generated_file).map_err(|error| {
            RunError::GenerateFailed {
                detail: format!("failed to write generated tests for {}: {error}", spec.name),
            }
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
        _binding: &BindingFile,
        generated: &GeneratedArtifact,
    ) -> Result<(), RunError> {
        if generated.spec_name.contains("build_error") {
            return Err(RunError::BuildFailed {
                detail: format!("mock backend could not build {}", generated.spec_name),
            });
        }

        let results = generated
            .cases
            .iter()
            .map(|case| CaseResult {
                name: case.raw_name.clone(),
                status: case_status(case),
                duration_ms: 1,
                traces_file: None,
                traces_match: None,
            })
            .collect::<Vec<_>>();

        let results_json = serde_json::to_string_pretty(&results)
            .expect("serializing CaseResult values should not fail");

        fs::write(&generated.results_path, results_json).map_err(|error| {
            RunError::BuildFailed {
                detail: format!(
                    "failed to write results for {}: {error}",
                    generated.spec_name
                ),
            }
        })?;

        Ok(())
    }

    fn collect_results(&self, generated: &GeneratedArtifact) -> Result<Vec<CaseResult>, RunError> {
        let results_json =
            fs::read_to_string(&generated.results_path).map_err(|error| RunError::BuildFailed {
                detail: format!(
                    "failed to read results for {}: {error}",
                    generated.spec_name
                ),
            })?;

        serde_json::from_str(&results_json).map_err(|error| RunError::BuildFailed {
            detail: format!(
                "failed to parse results for {}: {error}",
                generated.spec_name
            ),
        })
    }
}

fn case_status(case: &DiscoveredCase) -> CaseStatus {
    match case.mock_status.as_deref() {
        Some("fail") => CaseStatus::Fail,
        _ => CaseStatus::Pass,
    }
}

#[cfg(test)]
#[path = "mock_backend_tests.rs"]
mod tests;
