//! Separate CLI processes must not replace a package's active generated artifacts.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

struct RunningCapture {
    child: Option<Child>,
    release: PathBuf,
}

impl Drop for RunningCapture {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            fs::write(&self.release, b"release").expect("release the fixture test");
            if child.try_wait().expect("inspect the capture command").is_none() {
                child.kill().expect("stop the capture command");
            }
            child.wait().expect("reap the capture command");
        }
    }
}

fn completed(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(15);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("overlapping command did not fail immediately: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

fn command(action: &str, binding: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_specgate"));
    command
        .arg(action)
        .arg("--binding")
        .arg(binding)
        .arg("--impl")
        .arg("rust")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn commands_reject_an_owned_package_through_test_execution_and_release_it_afterward() {
    let cache = tempfile::tempdir().unwrap();
    let package = cache.path().join("candidate");
    fs::create_dir_all(package.join("src")).unwrap();
    let facade = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("specgate");
    let manifest = serde_json::json!({
        "package": {"name": "owned-package-fixture", "version": "0.0.0", "edition": "2024"},
        "dependencies": {"specgate": {"path": facade}},
        "workspace": {},
    });
    fs::write(package.join("Cargo.toml"), toml::to_string(&manifest).unwrap()).unwrap();
    fs::write(
        package.join("src").join("lib.rs"),
        r#"
specgate::spec_component!("fixture.stateless_add");
#[specgate::spec_operation("add")]
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[test]
fn captures_add() {
    let ready = std::env::var_os("SPECGATE_OWNERSHIP_READY").unwrap();
    let release = std::path::PathBuf::from(std::env::var_os("SPECGATE_OWNERSHIP_RELEASE").unwrap());
    std::fs::write(ready, b"ready").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
    while !release.exists() {
        assert!(std::time::Instant::now() < deadline, "fixture was not released");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(add(2, 3), 5);
}
"#,
    )
    .unwrap();
    let binding = cache.path().join("binding.yaml");
    fs::write(
        &binding,
        format!(
            "components:\n  fixture.stateless_add:\n    implementations:\n      rust:\n        language: rust\n        sources: '{}'\n        tests:\n          projects: ['{}']\n",
            package.display(),
            package.display()
        ),
    )
    .unwrap();
    let ready = cache.path().join("ready");
    let release = cache.path().join("release");
    let mut first = RunningCapture {
        child: Some(
            Command::new(env!("CARGO_BIN_EXE_specgate"))
                .args(["capture", "--component", "fixture.stateless_add", "--binding"])
                .arg(&binding)
                .args(["--impl", "rust", "--out"])
                .arg(cache.path().join("capture"))
                .env("SPECGATE_OWNERSHIP_READY", &ready)
                .env("SPECGATE_OWNERSHIP_RELEASE", &release)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        ),
        release: release.clone(),
    };
    let deadline = Instant::now() + Duration::from_mins(10);
    while !ready.exists() {
        assert!(Instant::now() < deadline, "capture never reached test execution");
        if first.child.as_mut().unwrap().try_wait().unwrap().is_some() {
            let output = first.child.take().unwrap().wait_with_output().unwrap();
            panic!("capture exited before executing its test: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let reference = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("..")
        .join("test")
        .join("behavior")
        .join("cases")
        .join("fixture.stateless_add")
        .join("expected");
    for action in ["discover", "capture", "replay"] {
        let out = cache.path().join(action);
        let mut second = command(action, &binding);
        match action {
            "discover" => {
                second.args([
                    "--component",
                    "fixture.stateless_add",
                    "--registry-id",
                    "urn:fixture:owned",
                    "--registry-version",
                    "1",
                ]);
            }
            "capture" => {
                second.args(["--component", "fixture.stateless_add"]);
            }
            "replay" => {
                second.arg(&reference);
            }
            _ => unreachable!(),
        }
        second.arg("--out").arg(&out);
        let output = completed(second.spawn().unwrap());
        assert_eq!(output.status.code(), Some(1), "{action}: {output:?}");
        let diagnostic = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            diagnostic.contains("package already in use by another SpecGate command"),
            "{action}: {diagnostic}"
        );
        assert!(!out.exists(), "{action} published output despite failing ownership acquisition");
    }
    fs::write(release, b"release").unwrap();
    let captured = first.child.take().unwrap().wait_with_output().unwrap();
    assert!(captured.status.success(), "{captured:?}");
    let captured_root = cache.path().join("capture");
    assert!(specgate_ctsc::validation::validate_bundle(&captured_root).valid);
    let replayed = Command::new(env!("CARGO_BIN_EXE_specgate"))
        .arg("replay")
        .arg(&captured_root)
        .arg("--binding")
        .arg(&binding)
        .args(["--impl", "rust", "--out"])
        .arg(cache.path().join("after-release.json"))
        .output()
        .unwrap();
    assert!(replayed.status.success(), "{replayed:?}");
}
