//! Compiler-reported Cargo test targets and annotation-generated entry points.

use super::system::{ProcessRequest, System};
use crate::{Error, discovery::registry::TestTarget, error::ErrorKind};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// One compiler-reported test executable with an internal annotation hook.
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

/// Build and enumerate only annotation-generated entry points.
///
/// # Errors
/// Reports Cargo, metadata, and libtest enumeration failures.
pub fn build(package: &Path, entry: &Path, dispatch: &Path, scratch: &Path, target: Option<&TestTarget>) -> Result<Vec<Binary>, Error> {
    build_in(package, entry, Some(dispatch), scratch, target, &System::real())
}

fn build_in(
    package: &Path,
    entry: &Path,
    dispatch: Option<&Path>,
    scratch: &Path,
    target: Option<&TestTarget>,
    system: &System,
) -> Result<Vec<Binary>, Error> {
    entry.to_str().ok_or_else(|| failure("test-harness entry path is not Unicode"))?;
    let request = ProcessRequest::builder(super::cargo_with(system)).arg("test");
    let request = match target {
        Some(target) if target.library => request.arg("--lib"),
        Some(target) => request.arg("--test").arg(&target.name),
        None => request.arg("--tests"),
    };
    let request = match dispatch {
        Some(dispatch) => request.env("SPECGATE_TEST_DISPATCH", dispatch),
        None => request.env_remove("SPECGATE_TEST_DISPATCH"),
    };
    let request = request
        .arg("--no-run")
        .arg("--quiet")
        .arg("--message-format=json")
        .current_dir(package)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("SPECGATE_TEST_ENTRY", entry)
        .env("CARGO_TARGET_DIR", scratch.join("target"))
        .build();
    let output = system.process.output(&request)?;
    if !output.success {
        let diagnostics = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let message: Value = serde_json::from_str(line).ok()?;
                message["message"]["rendered"].as_str().map(str::to_string)
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Err(failure(format!(
            "test-harness build failed: {}\n{diagnostics}",
            String::from_utf8_lossy(&output.stderr).trim(),
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
        let library = target["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().filter_map(Value::as_str).any(super::metadata::library_kind));
        let request = ProcessRequest::builder(executable)
            .arg("--list")
            .arg("--format")
            .arg("terse")
            .build();
        let listed = system.process.output(&request)?;
        if !listed.success {
            return Err(failure(format!(
                "test-harness enumeration failed for '{name}': {}",
                String::from_utf8_lossy(&listed.stderr)
            )));
        }
        let entry = String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter_map(|line| line.strip_suffix(": test"))
            .find(|name| name.split("::").last().is_some_and(|leaf| leaf.starts_with("__SPECGATE_harness_")))
            .map(str::to_string);
        if let Some(entry) = entry {
            binaries.push(Binary {
                target: TestTarget { name, library },
                executable: PathBuf::from(executable),
                entry,
            });
        }
    }
    binaries.sort_by(|left, right| {
        right
            .target
            .library
            .cmp(&left.target.library)
            .then_with(|| left.target.name.cmp(&right.target.name))
    });
    Ok(binaries)
}

pub(crate) fn augment(package: &Path, original: &str, scratch: &Path, system: &System) -> Result<String, Error> {
    let entry = scratch.join("test-entry.rs");
    system.filesystem.write(&entry, b"{ let path = std::env::var_os(\"SPECGATE_TEST_METADATA\").expect(\"missing metadata output\"); std::fs::write(path, __specgate::discovery().to_string()).expect(\"failed to write test-target metadata\"); }")?;
    let binaries = build_in(package, &entry, None, &scratch.join("tests"), None, system)?;
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
        let output = system.process.output(&request)?;
        if !output.success {
            return Err(failure(format!(
                "test-target metadata hook failed: {}",
                String::from_utf8_lossy(&output.stderr)
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
