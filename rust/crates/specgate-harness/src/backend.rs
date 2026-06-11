use std::path::{Path, PathBuf};

use specgate_types::{BindingFile, CaseResult, RunError, SpecDocument};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredCase {
    pub raw_name: String,
    /// Mock backends can use this to control pass/fail behavior.
    pub mock_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    pub cases: Vec<DiscoveredCase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedArtifact {
    pub generated_test_path: PathBuf,
    pub results_path: PathBuf,
    pub cases: Vec<DiscoveredCase>,
    pub spec_name: String,
}

pub trait Backend: Send + Sync {
    fn build_and_discover(
        &self,
        binding: &BindingFile,
        spec: &SpecDocument,
    ) -> Result<Discovery, RunError>;

    fn generate(
        &self,
        binding: &BindingFile,
        spec: &SpecDocument,
        discovery: &Discovery,
        workdir: &Path,
    ) -> Result<GeneratedArtifact, RunError>;

    fn run_command(
        &self,
        binding: &BindingFile,
        generated: &GeneratedArtifact,
    ) -> Result<(), RunError>;

    fn collect_results(&self, generated: &GeneratedArtifact) -> Result<Vec<CaseResult>, RunError>;
}
