//! Fake-system cache and metadata process tests.

use super::*;

#[test]
fn cache_uniqueness() {
    let filesystem = system::FakeFs::builder()
        .unique_candidates(
            [
                Err("collision".to_string()),
                Ok(PathBuf::from("cache/one")),
                Ok(PathBuf::from("cache/two")),
            ]
            .into(),
        )
        .build();
    let system = fake_system(
        filesystem,
        Vec::new(),
        BTreeMap::from([("SPECGATE_CACHE_DIR".to_string(), "cache-root".into())]),
    );
    let state = system.filesystem.fake_state();
    let first = InvocationCache::create_with(CacheScope::new("same scope"), CacheLabel::new("same label"), &system).unwrap();
    let second = InvocationCache::create_with(CacheScope::new("same scope"), CacheLabel::new("same label"), &system).unwrap();
    assert_ne!(first.path(), second.path());
    drop(first);
    assert_eq!(state.borrow().removed(), vec![PathBuf::from("cache/one")]);
}

#[test]
fn cache_root() {
    let environment = BTreeMap::from([
        ("SPECGATE_CACHE_DIR".to_string(), "explicit".into()),
        ("LOCALAPPDATA".to_string(), "platform".into()),
        ("HOME".to_string(), "home".into()),
    ]);
    let system = fake_system(
        system::FakeFs::builder()
            .unique_candidates([Err("denied".to_string())].into())
            .build(),
        Vec::new(),
        environment,
    );
    assert_eq!(root_for(&system).unwrap(), PathBuf::from("explicit"));
    assert!(
        InvocationCache::create_with(CacheScope::new("scope"), CacheLabel::new("label"), &system)
            .unwrap_err()
            .contains("denied")
    );
    let empty = fake_system(system::FakeFs::default(), Vec::new(), BTreeMap::new());
    assert_eq!(
        root_for(&empty).unwrap_err(),
        "no operating-system cache directory is available; set SPECGATE_CACHE_DIR"
    );
}

#[test]
fn metadata_failures() {
    let base_fs = || {
        system::FakeFs::builder()
            .canonical(BTreeMap::from([(
                PathBuf::from("candidate/Cargo.toml"),
                Ok(PathBuf::from("candidate/Cargo.toml")),
            )]))
            .build()
    };
    for (result, expected) in [
        (Err("spawn denied".to_string()), "failed to invoke cargo metadata: spawn denied"),
        (
            Ok(system::ProcessOutput {
                success: false,
                stdout: Vec::new(),
                stderr: b"bad graph\n".to_vec(),
            }),
            "cargo metadata failed: bad graph",
        ),
        (
            Ok(system::ProcessOutput {
                success: true,
                stdout: b"{".to_vec(),
                stderr: Vec::new(),
            }),
            "cargo metadata output was malformed:",
        ),
    ] {
        let system = fake_system(
            base_fs(),
            vec![result],
            BTreeMap::from([("CARGO".to_string(), "custom-cargo".into())]),
        );
        let error = candidate_in(Path::new("candidate"), &system).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert_eq!(
            system.process.fake_state().lock().unwrap().requests[0].executable,
            OsString::from("custom-cargo")
        );
    }
}

#[test]
fn metadata_identity() {
    let filesystem = system::FakeFs::builder()
        .canonical(BTreeMap::from([(
            PathBuf::from("candidate/Cargo.toml"),
            Ok(PathBuf::from("candidate/Cargo.toml")),
        )]))
        .build();
    let system = fake_system(
        filesystem,
        vec![Ok(system::ProcessOutput {
            success: true,
            stdout: metadata_json(),
            stderr: Vec::new(),
        })],
        BTreeMap::new(),
    );
    let context = candidate_in(Path::new("candidate"), &system).unwrap();
    assert_eq!(context.package, "candidate");
    assert_eq!(context.runtime.version, "0.6.0");
    let request = system.process.fake_state().lock().unwrap().requests[0].clone();
    assert_eq!(request.current_dir.as_deref(), Some(Path::new("candidate")));
    assert!(
        request
            .args
            .windows(2)
            .any(|args| args == ["--manifest-path", "candidate/Cargo.toml"])
    );
}
