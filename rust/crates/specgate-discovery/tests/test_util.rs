//! Deterministic discovery adapter integration tests.

#![cfg(feature = "test-util")]

use specgate_discovery::binding::{Implementation, LanguageOptions, ResolvedImplementation, Runtime, Tests};
use specgate_discovery::identity::{ComponentId, ImplementationLabel};
use specgate_discovery::test_util::{Binding, Discovery, FailurePoint, FileOperation};
use std::path::PathBuf;

fn rust_target() -> ResolvedImplementation {
    ResolvedImplementation {
        binding_path: PathBuf::from("binding.yaml"),
        component: ComponentId::from("demo.component"),
        label: ImplementationLabel::from("main"),
        implementation: Implementation::new(
            "candidate",
            Tests::Command("cargo test".to_string()),
            LanguageOptions::Rust { runtime: Runtime::Smol },
        )
        .unwrap(),
    }
}

#[test]
fn binding_harness_loads_without_real_filesystem_access() {
    let binding = Binding::builder(
        "components:\n  demo.component:\n    implementations:\n      main:\n        language: rust\n        sources: fixture\n        tests:\n          command: cargo test\n",
    )
    .canonical_path("virtual/binding.yaml")
    .build()
    .load("missing/binding.yaml")
    .unwrap();
    assert_eq!(binding.path, PathBuf::from("virtual/binding.yaml"));
    let resolved = binding.resolve_implementation("demo.component", None).unwrap();
    assert_eq!(resolved.implementation.sources, PathBuf::from("virtual/fixture"));
}

#[test]
fn binding_harness_injects_read_failure() {
    let error = Binding::builder("")
        .canonicalization_failure("canonicalization denied")
        .read_failure("read denied")
        .build()
        .load("missing/binding.yaml")
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("binding 'missing/binding.yaml' not found or invalid: read denied")
    );
}

#[test]
fn discovery_harness_injects_filesystem_failure() {
    let error = Discovery::builder()
        .filesystem_failure(
            FailurePoint::file(FileOperation::Canonicalize, "Cargo.toml").unwrap(),
            "manifest denied",
        )
        .build()
        .discover(rust_target(), &[ComponentId::from("demo.component")])
        .unwrap_err();
    assert!(error.to_string().contains("cannot resolve candidate Cargo.toml: manifest denied"));
}

#[test]
fn discovery_harness_injects_process_failure() {
    let error = Discovery::builder()
        .process_failure("cargo unavailable")
        .build()
        .discover(rust_target(), &[ComponentId::from("demo.component")])
        .unwrap_err();
    assert!(error.to_string().contains("failed to invoke cargo metadata: cargo unavailable"));
}
