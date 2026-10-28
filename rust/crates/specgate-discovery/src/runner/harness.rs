//! Compiler-reported Cargo test targets and CLI-injected crate-root entry points.

use super::system::{ProcessRequest, System};
use crate::{
    Error,
    discovery::registry::{TestTarget, TestTargetKind},
    error::ErrorKind,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

mod compiler_build;

#[cfg(test)]
mod compiler {
    const ROOTS: &[&str] = &[];
    const FACADES: &[&str] = &[];
    include!("harness/compiler.rs");
}

/// One compiler-reported test executable with an internal crate-root hook.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Binary {
    /// Cargo target identity.
    pub target: TestTarget,
    /// Compiler-reported executable, never inferred from a source layout.
    pub executable: PathBuf,
    /// Exact generated libtest entry name.
    pub entry: String,
}

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Cargo, message)
}

/// Build and enumerate only CLI-injected entry points.
///
/// # Errors
/// Reports Cargo, metadata, and libtest enumeration failures.
pub fn build(
    package: &Path,
    entry: &Path,
    dispatch: &Path,
    module_dispatches: &BTreeMap<String, PathBuf>,
    scratch: &Path,
    target: Option<&TestTarget>,
) -> Result<Vec<Binary>, Error> {
    build_in(package, entry, Some(dispatch), module_dispatches, scratch, target, &System::real())
}

fn build_in(
    package: &Path,
    entry: &Path,
    dispatch: Option<&Path>,
    module_dispatches: &BTreeMap<String, PathBuf>,
    scratch: &Path,
    target: Option<&TestTarget>,
    system: &System,
) -> Result<Vec<Binary>, Error> {
    let wrapper = compiler_build::build(package, scratch, system)?;
    build_with_wrapper(package, entry, dispatch, module_dispatches, scratch, target, &wrapper, system)
}

#[expect(
    clippy::too_many_arguments,
    reason = "test compiler and concrete system adapters are injected independently"
)]
fn build_with_wrapper(
    package: &Path,
    entry: &Path,
    dispatch: Option<&Path>,
    module_dispatches: &BTreeMap<String, PathBuf>,
    scratch: &Path,
    target: Option<&TestTarget>,
    compiler: &compiler_build::Compiler,
    system: &System,
) -> Result<Vec<Binary>, Error> {
    entry.to_str().ok_or_else(|| failure("test-harness entry path is not Unicode"))?;
    let request = ProcessRequest::builder(super::cargo_with(system)).arg("test");
    let request = match target {
        Some(TestTarget {
            kind: TestTargetKind::Library,
            ..
        }) => request.arg("--lib"),
        Some(TestTarget {
            kind: TestTargetKind::Binary,
            name,
        }) => request.arg("--bin").arg(name),
        Some(TestTarget {
            kind: TestTargetKind::Integration,
            name,
        }) => request.arg("--test").arg(name),
        None => request.arg("--tests"),
    };
    let request = match dispatch {
        Some(dispatch) => request.env("SPECGATE_TEST_DISPATCH", dispatch),
        None => request.env_remove("SPECGATE_TEST_DISPATCH"),
    };
    let request = module_dispatches.iter().fold(request, |request, (module, path)| {
        request.env(format!("SPECGATE_TEST_MODULE_{module}"), path)
    });
    let request = match &compiler.build_directory {
        // Cargo omits filename hashes for C-compatible library artifacts.
        // Wrapper-specific builds must not overwrite ordinary cached rlibs.
        Some(path) => request.env("CARGO_BUILD_BUILD_DIR", path),
        None => request,
    };
    let request = request
        .arg("--no-run")
        .arg("--quiet")
        .arg("--message-format=json")
        .current_dir(package)
        .env("RUSTC_WORKSPACE_WRAPPER", &compiler.executable)
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("SPECGATE_TEST_ENTRY", entry)
        // Environment-dependent macro expansion must not reuse incremental state.
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .build();
    let output = system
        .output(&request)
        .map_err(|source| Error::cause(ErrorKind::Cargo, "failed to run test-harness build", source))?;
    if !output.status.success() {
        return Err(failure(format!(
            "test-harness build failed:\n{}",
            request.diagnostic(Some(&output), &system.environment),
        )));
    }
    let mut binaries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        let Some(executable) = message["executable"].as_str() else {
            continue;
        };
        let target = &message["target"];
        let name = target["name"]
            .as_str()
            .ok_or_else(|| failure("test artifact has no target name"))?
            .to_string();
        let kinds = target["kind"]
            .as_array()
            .ok_or_else(|| failure("test artifact has no target kind"))?;
        let kind = if kinds.iter().filter_map(Value::as_str).any(super::metadata::library_kind) {
            TestTargetKind::Library
        } else if kinds.iter().any(|kind| kind == "bin") {
            TestTargetKind::Binary
        } else if kinds.iter().any(|kind| kind == "test") {
            TestTargetKind::Integration
        } else {
            return Err(failure(format!("unsupported Cargo test target kind for '{name}': {kinds:?}")));
        };
        let request = ProcessRequest::builder(executable)
            .arg("--list")
            .arg("--format")
            .arg("terse")
            .build();
        let listed = system.output(&request).map_err(|source| {
            Error::cause(
                ErrorKind::Cargo,
                format!("failed to enumerate test-harness binary '{name}'"),
                source,
            )
        })?;
        if !listed.status.success() {
            return Err(failure(format!(
                "test-harness enumeration failed for '{name}': {}",
                request.diagnostic(Some(&listed), &system.environment)
            )));
        }
        let entry = String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter_map(|line| line.strip_suffix(": test"))
            .find(|name| name.split("::").last().is_some_and(|leaf| leaf.starts_with("__SPECGATE_harness_")))
            .map(str::to_string);
        if let Some(entry) = entry {
            binaries.push(Binary {
                target: TestTarget { name, kind },
                executable: PathBuf::from(executable),
                entry,
            });
        }
    }
    binaries.sort_by(|left, right| {
        left.target
            .kind
            .cmp(&right.target.kind)
            .then_with(|| left.target.name.cmp(&right.target.name))
    });
    Ok(binaries)
}

pub(crate) fn augment(package: &Path, original: &str, scratch: &Path, system: &System) -> Result<String, Error> {
    let entry = scratch.join("test-entry.rs");
    system.filesystem.write(&entry, b"{ let path = std::env::var_os(\"SPECGATE_TEST_METADATA\").expect(\"missing metadata output\"); std::fs::write(path, __specgate::discovery().to_string()).expect(\"failed to write test-target metadata\"); }")?;
    let binaries = build_in(package, &entry, None, &BTreeMap::new(), &scratch.join("tests"), None, system)?;
    let mut document: Value = serde_json::from_str(original).map_err(|error| failure(format!("invalid production registry: {error}")))?;
    for (index, binary) in binaries.iter().enumerate() {
        let metadata = scratch.join(format!("test-metadata-{index}.json"));
        let request = ProcessRequest::builder(&binary.executable)
            .arg(&binary.entry)
            .arg("--exact")
            .arg("--test-threads=1")
            .env("SPECGATE_TEST_METADATA", &metadata)
            .env_remove("SPECGATE_NATIVE_CAPTURE")
            .build();
        let output = system
            .output(&request)
            .map_err(|source| Error::cause(ErrorKind::Cargo, "failed to run test-target metadata hook", source))?;
        if !output.status.success() {
            return Err(failure(format!(
                "test-target metadata hook failed:\n{}",
                request.diagnostic(Some(&output), &system.environment)
            )));
        }
        let text = system.filesystem.read_to_string(&metadata)?;
        let found: Value = serde_json::from_str(&text).map_err(|error| failure(format!("invalid test-target registry: {error}")))?;
        for key in ["operations", "types"] {
            let entries = found[key]
                .as_array()
                .ok_or_else(|| failure(format!("test-target registry has no '{key}' array")))?;
            let combined = document[key]
                .as_array_mut()
                .ok_or_else(|| failure(format!("production registry has no '{key}' array")))?;
            for value in entries {
                let mut value = value.clone();
                let existing = combined.iter_mut().find(|prior| {
                    ["component", "name", "module_path", "fn_name", "is_setup", "fills", "type_arguments"]
                        .iter()
                        .all(|field| prior[*field] == value[*field])
                });
                if let Some(prior) = existing {
                    if prior["is_setup"] == false && prior.get("test_target").is_none() {
                        prior["test_target"] = serde_json::to_value(&binary.target).map_err(|error| failure(error.to_string()))?;
                    }
                    if let Some(output) = value.get("replay_output") {
                        prior["replay_output"] = output.clone();
                        if prior["is_setup"] == true && prior["is_public"] == false && prior.get("test_target").is_none() {
                            prior["test_target"] = serde_json::to_value(&binary.target).map_err(|error| failure(error.to_string()))?;
                        }
                    }
                    let mut left = prior.clone();
                    let mut right = value;
                    for ignored in ["test_target", "replay_output"] {
                        left.as_object_mut().expect("registry entry object").remove(ignored);
                        right.as_object_mut().expect("registry entry object").remove(ignored);
                    }
                    if left != right {
                        return Err(failure(
                            "production and test-target metadata disagree for the same native declaration",
                        ));
                    }
                } else {
                    if key == "operations" {
                        value["test_target"] = serde_json::to_value(&binary.target).map_err(|error| failure(error.to_string()))?;
                    }
                    combined.push(value);
                }
            }
        }
    }
    serde_json::to_string(&document).map_err(|error| failure(format!("failed to encode combined test-target registry: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::system::{FakeFs, FakeProcess, ProcessOutput};
    use std::ffi::OsStr;

    #[test]
    fn unhashed_library_builds_use_a_reusable_separate_namespace() {
        let system = System::builder((
            FakeFs::default(),
            FakeProcess::queued([Ok(ProcessOutput {
                status: crate::runner::system::test_status(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })]),
        ))
        .build();
        let namespace = Path::new("shared").join("specgate-harness");
        build_with_wrapper(
            Path::new("candidate"),
            Path::new("entry.rs"),
            None,
            &BTreeMap::new(),
            Path::new("scratch"),
            None,
            &compiler_build::Compiler {
                executable: PathBuf::from("compiler"),
                build_directory: Some(namespace.clone()),
            },
            &system,
        )
        .unwrap();
        let state = system.process.fake_state();
        let state = state.lock().unwrap();
        assert!(
            state.requests[0]
                .env
                .iter()
                .any(|(key, value)| key == "CARGO_BUILD_BUILD_DIR" && value == namespace.as_os_str())
        );
    }

    #[test]
    fn generated_builds_disable_incremental_without_replacing_shared_dependencies_or_flags() {
        let system = System::builder((
            FakeFs::default(),
            FakeProcess::queued((0..2).map(|_| {
                Ok(ProcessOutput {
                    status: crate::runner::system::test_status(0),
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                })
            })),
        ))
        .build();
        for dispatch in [None, Some(Path::new("dispatch.rs"))] {
            build_with_wrapper(
                Path::new("candidate"),
                Path::new("entry.rs"),
                dispatch,
                &BTreeMap::new(),
                Path::new("scratch"),
                None,
                &compiler_build::Compiler {
                    executable: PathBuf::from("compiler"),
                    build_directory: None,
                },
                &system,
            )
            .unwrap();
        }
        let state = system.process.fake_state();
        let state = state.lock().unwrap();
        assert_eq!(state.requests.len(), 2);
        for request in &state.requests {
            assert!(request.env.iter().any(|(key, value)| key == "CARGO_INCREMENTAL" && value == "0"));
            for preserved in ["CARGO_BUILD_BUILD_DIR", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
                assert!(!request.env.iter().any(|(key, _)| key == preserved));
                assert!(!request.env_remove.contains(&OsStr::new(preserved).to_os_string()));
            }
        }
    }
}
