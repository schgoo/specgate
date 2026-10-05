//! Generated manifest and package-source integration tests.

use super::*;

#[test]
fn manifest_escaping() {
    let mut dependencies = BTreeMap::new();
    dependencies.insert(
        "candidate alias".to_string(),
        Dependency::builder("candidate\"quoted")
            .version("=0.6.0+metadata")
            .path("path\"quoted\\segment")
            .build()
            .unwrap(),
    );
    let cargo = runner_cargo("runner\"quoted", dependencies).unwrap();
    let parsed: toml::Value = toml::from_str(&cargo.manifest).unwrap();
    assert_eq!(parsed["package"]["name"].as_str(), Some("runner\"quoted"));
    assert_eq!(
        parsed["dependencies"]["candidate alias"]["package"].as_str(),
        Some("candidate\"quoted")
    );
    assert_eq!(cargo.config, None);
    #[cfg(windows)]
    assert_eq!(
        parsed["dependencies"]["candidate alias"]["path"].as_str(),
        Some("path\"quoted/segment")
    );
    #[cfg(not(windows))]
    assert_eq!(
        parsed["dependencies"]["candidate alias"]["path"].as_str(),
        Some("path\"quoted\\segment")
    );
}

#[test]
fn candidate_root() {
    let cache = InvocationCache::create(CacheScope::new("tests"), CacheLabel::new("metadata-source")).unwrap();
    let runtime = cache.path().join("runtime");
    let candidate = cache.path().join("candidate");
    std::fs::create_dir_all(runtime.join("src")).unwrap();
    std::fs::create_dir_all(candidate.join("src")).unwrap();
    std::fs::write(
        runtime.join("Cargo.toml"),
        "[package]\nname=\"specgate-runtime\"\nversion=\"0.6.0\"\nedition=\"2024\"\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(runtime.join("src/lib.rs"), "").unwrap();
    std::fs::write(
            candidate.join("Cargo.toml"),
            "[package]\nname=\"candidate\"\nversion=\"0.1.0\"\nedition=\"2024\"\n[dependencies]\nspecgate-runtime={path=\"../runtime\"}\n[workspace]\n",
        )
        .unwrap();
    std::fs::write(candidate.join("src/lib.rs"), "").unwrap();

    let context = candidate_package(&candidate).unwrap();
    assert_eq!(context.runtime.path(), Some(runtime.as_path()));
    assert!(
        !context
            .runtime
            .path()
            .unwrap()
            .starts_with(std::env::current_dir().unwrap().join("rust").join("crates"))
    );
}

#[test]
fn path_mismatch() {
    let project = TestProject::create("mismatched-runtime-path");
    let candidate_runtime = project.path().join("candidate-runtime");
    let override_runtime = project.path().join("override-runtime");
    let candidate = project.path().join("candidate");
    path_runtime(&candidate_runtime, "0.6.0");
    path_runtime(&override_runtime, "0.6.0");
    path_candidate(&candidate, &candidate_runtime);

    let override_ = local_override(&override_runtime).unwrap();
    let error = context_override(&candidate, Some(&override_)).unwrap_err();

    assert!(error.contains("SPECGATE_RUNTIME_PATH"));
    assert!(error.contains("different Cargo PackageId"));
    assert!(error.contains("remove the override or align the candidate dependency"));
}

#[test]
fn version_mismatch() {
    let project = TestProject::create("mismatched-runtime-version");
    let runtime = project.path().join("runtime");
    let candidate = project.path().join("candidate");
    path_runtime(&runtime, "0.6.0");
    path_candidate(&candidate, &runtime);

    let error = context_override(&candidate, Some(&RuntimeOverride::Version("0.7.0".to_string()))).unwrap_err();

    assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.7.0"));
    assert!(error.contains("candidate resolves specgate-runtime version 0.6.0"));
    assert!(error.contains("remove the override or align the candidate dependency"));
}

#[test]
fn source_mismatch() {
    let project = TestProject::create("matching-version-path-identity");
    let runtime = project.path().join("runtime");
    let candidate = project.path().join("candidate");
    path_runtime(&runtime, "0.6.0");
    path_candidate(&candidate, &runtime);

    let error = context_override(&candidate, Some(&RuntimeOverride::Version("0.6.0".to_string()))).unwrap_err();

    assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.6.0 selects crates.io"));
    assert!(error.contains("distinct runtime/linkme/capture state"));
    assert!(error.contains("remove the override"));
}

#[test]
fn lazy_override() {
    let runtime = CargoPackage {
        id: "git+https://example.invalid/specgate#specgate-runtime@0.6.0".to_string(),
        name: "specgate-runtime".to_string(),
        version: "0.6.0".to_string(),
        manifest_path: PathBuf::from("checkout/specgate-runtime/Cargo.toml"),
        source: Some("git+https://example.invalid/specgate".to_string()),
    };

    let error = override_source(&runtime, &RuntimeOverride::Version("0.6.0".to_string())).unwrap_err();

    assert!(error.contains("SPECGATE_RUNTIME_VERSION=0.6.0 selects crates.io"));
    assert!(!error.contains("unsupported specgate-runtime Cargo source"));
}

#[test]
fn path_capture() {
    let project = TestProject::create("matching-runtime-path");
    let runtime = project.path().join("runtime");
    let candidate = project.path().join("candidate");
    let runner = project.path().join("runner");
    path_runtime(&runtime, "0.6.0");
    path_candidate(&candidate, &runtime);

    let override_ = local_override(&runtime).unwrap();
    let context = context_override(&candidate, Some(&override_)).unwrap();

    assert_eq!(context.runtime.path(), Some(runtime.as_path()));
    run_capture(&candidate, &runner, &context);
}

#[test]
fn registry_source() {
    for source in [
        "registry+https://github.com/rust-lang/crates.io-index",
        "registry+https://registry.example.invalid/index",
        "registry+sparse+https://registry.example.invalid/index/",
        "sparse+https://registry.example.invalid/index/",
    ] {
        let package = CargoPackage {
            id: format!("registry-package#{source}"),
            name: "specgate-runtime".to_string(),
            version: "0.6.0".to_string(),
            manifest_path: PathBuf::from("cargo-home")
                .join("registry")
                .join("src")
                .join("resolved-source")
                .join("specgate-runtime-0.6.0")
                .join("Cargo.toml"),
            source: Some(source.to_string()),
        };
        let resolved = package_source(&package).unwrap();
        assert_eq!(resolved.path(), None);
        assert_eq!(resolved.registry_source(), Some(source));
    }
}

#[test]
fn registry_alias() {
    for (source, expected_index) in [
        (
            "registry+https://registry.example.invalid/git-index",
            "https://registry.example.invalid/git-index",
        ),
        (
            "registry+sparse+https://registry.example.invalid/sparse-index/",
            "sparse+https://registry.example.invalid/sparse-index/",
        ),
        (
            "sparse+https://registry.example.invalid/direct-sparse-index/",
            "sparse+https://registry.example.invalid/direct-sparse-index/",
        ),
    ] {
        let dependency = Dependency::from_source(&PackageSource::registry("specgate-runtime", "0.6.0", source)).unwrap();
        let first = runner_cargo(
            "alternate-registry-runner",
            BTreeMap::from([("specgate_runtime".to_string(), dependency.clone())]),
        )
        .unwrap();
        let second = runner_cargo(
            "alternate-registry-runner",
            BTreeMap::from([("specgate_runtime".to_string(), dependency)]),
        )
        .unwrap();
        assert_eq!(first, second);

        let manifest: toml::Value = toml::from_str(&first.manifest).unwrap();
        let alias = manifest["dependencies"]["specgate_runtime"]["registry"].as_str().unwrap();
        assert_eq!(manifest["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
        assert!(manifest["dependencies"]["specgate_runtime"].get("path").is_none());
        let config: toml::Value = toml::from_str(first.config.as_deref().unwrap()).unwrap();
        assert_eq!(config["registries"][alias]["index"].as_str(), Some(expected_index));
    }
}

#[test]
fn crates_io_version() {
    for source in [
        "registry+https://github.com/rust-lang/crates.io-index",
        "sparse+https://index.crates.io/",
    ] {
        let cargo = runner_cargo(
            "crates-io-runner",
            BTreeMap::from([(
                "specgate_runtime".to_string(),
                Dependency::from_source(&PackageSource::registry("specgate-runtime", "0.6.0", source)).unwrap(),
            )]),
        )
        .unwrap();
        let manifest: toml::Value = toml::from_str(&cargo.manifest).unwrap();
        assert_eq!(manifest["dependencies"]["specgate_runtime"]["version"].as_str(), Some("=0.6.0"));
        assert!(manifest["dependencies"]["specgate_runtime"].get("path").is_none());
        assert!(manifest["dependencies"]["specgate_runtime"].get("registry").is_none());
        assert_eq!(cargo.config, None);
    }
}

#[test]
fn replacement_capture() {
    let project = TestProject::create("registry-source-identity");
    let registry = project.path().join("registry");
    let runtime = project.path().join("runtime");
    let candidate = project.path().join("candidate");
    let runner = project.path().join("runner");
    let (index_entry, archive) = package_archive(&runtime);
    create_registry(&registry, &index_entry, &archive);
    registry_candidate(&candidate, &registry);

    let context = context_override(&candidate, Some(&RuntimeOverride::Version(RUNTIME_VERSION.to_string()))).unwrap();
    assert_eq!(context.runtime.path(), None);
    assert_eq!(
        context.runtime.registry_source(),
        Some("registry+https://github.com/rust-lang/crates.io-index")
    );
    run_capture(&candidate, &runner, &context);
}

#[test]
fn sparse_capture() {
    let project = TestProject::create("alternate-registry-identity");
    let runtime = project.path().join("runtime");
    let candidate = project.path().join("candidate");
    let runner = project.path().join("runner");
    let (index_entry, archive) = package_archive(&runtime);
    let registry = SparseRegistry::start(index_entry, archive);
    alternate_candidate(&candidate, registry.index());

    let context = candidate_package(&candidate).unwrap();
    assert_eq!(context.runtime.path(), None);
    assert_eq!(context.runtime.registry_source(), Some(registry.index()));
    run_capture(&candidate, &runner, &context);
}
