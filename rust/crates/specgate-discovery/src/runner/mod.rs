//! Generated-runner Cargo identity, process execution, and cache support.
//!
//! Candidate metadata is always rooted at the candidate package. Generated
//! manifests preserve the exact resolved `specgate-runtime` package source,
//! while process and filesystem access flows through one concrete system
//! composition shared by real and fake adapters.

pub(crate) mod cache;
mod metadata;
pub(crate) mod system;

pub use crate::discovery::runner::{cargo_bin, run_discovery};
pub use metadata::{
    CandidateDeps, CandidatePackage, Dependency, DependencyBuilder, PackageSource, RunnerCargo, candidate_package, runner_cargo,
};
pub(crate) use metadata::{candidate_in, cargo_with};
