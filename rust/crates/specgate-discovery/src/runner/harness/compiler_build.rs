//! Build the dependency-free host helper used to place a test at the crate root.

use super::{ProcessRequest, System, failure};
use crate::Error;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) struct Compiler {
    pub executable: PathBuf,
    pub build_directory: Option<PathBuf>,
}

pub(super) fn build(package: &Path, scratch: &Path, system: &System) -> Result<Compiler, Error> {
    let config = crate::runner::metadata::harness_config(package, &scratch.join("target"), system)?;
    let wrapper = system.cargo_config.workspace_wrapper(package)?;
    let wrapper = wrapper
        .as_deref()
        .map(|path| {
            path.to_str()
                .ok_or_else(|| failure("configured Cargo workspace wrapper is not Unicode"))
        })
        .transpose()?;
    let directory = scratch.join("compiler");
    system.filesystem.create_dir_all(directory.join("src"))?;
    let cargo = crate::runner::runner_cargo("specgate-test-compiler", BTreeMap::new())?;
    system.filesystem.write(directory.join("Cargo.toml"), cargo.manifest)?;
    let roots = config
        .roots
        .iter()
        .map(|path| path.to_str().ok_or_else(|| failure("test crate root is not Unicode")))
        .collect::<Result<Vec<_>, _>>()?;
    let source = format!(
        "const ROOTS: &[&str] = &{roots:?};\nconst FACADES: &[&str] = &{:?};\n\
         const SCRATCH: &str = {:?};\nconst WRAPPER: Option<&str> = {wrapper:?};\ninclude!({:?});\n",
        config.facades,
        directory
            .join("roots")
            .to_str()
            .ok_or_else(|| failure("compiler scratch path is not Unicode"))?,
        "compiler.rs"
    );
    system.filesystem.write(directory.join("src").join("main.rs"), source)?;
    system
        .filesystem
        .write(directory.join("src").join("compiler.rs"), include_str!("compiler.rs"))?;
    let request = ProcessRequest::builder(crate::runner::cargo_with(system))
        .arg("build")
        .arg("--quiet")
        .arg("--message-format=json")
        .arg("--manifest-path")
        .arg(directory.join("Cargo.toml"))
        .current_dir(package)
        .env_remove("SPECGATE_TEST_ENTRY")
        .env_remove("SPECGATE_TEST_DISPATCH")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .build();
    let output = system.process.output(&request)?;
    if !output.success {
        return Err(failure(format!(
            "test-harness compiler build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let executable = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|message| {
            (message["reason"] == "compiler-artifact" && message["target"]["name"] == "specgate-test-compiler")
                .then(|| message["executable"].as_str().map(PathBuf::from))
                .flatten()
        })
        .ok_or_else(|| failure("Cargo did not report the test-harness compiler executable"))?;
    Ok(Compiler {
        executable,
        build_directory: config.build_directory,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::system::{FakeFs, FakeProcess, ProcessOutput};
    use std::ffi::OsStr;

    #[test]
    fn configured_workspace_wrapper_is_embedded_without_clearing_cargo_wrappers() {
        let package = Path::new("candidate");
        let manifest = package.join("Cargo.toml");
        let scratch = Path::new("scratch");
        for wrapper in [None, Some(PathBuf::from("tools").join("workspace wrapper"))] {
            let metadata = serde_json::json!({
                "packages": [{
                    "id": "facade", "name": "specgate", "version": "0.6.0",
                    "manifest_path": manifest,
                    "targets": [{"name": "specgate", "kind": ["lib"], "src_path": package.join("src").join("lib.rs")}]
                }]
            });
            let artifact = serde_json::json!({
                "reason": "compiler-artifact",
                "target": {"name": "specgate-test-compiler"},
                "executable": "compiler-helper"
            });
            let builder = System::builder((
                FakeFs::default(),
                FakeProcess::queued([metadata, artifact].map(|document| {
                    Ok(ProcessOutput {
                        success: true,
                        stdout: serde_json::to_vec(&document).unwrap(),
                        stderr: Vec::new(),
                    })
                })),
            ));
            let system = match &wrapper {
                Some(wrapper) => builder.workspace_wrapper(wrapper),
                None => builder,
            }
            .build();
            let compiler = build(package, scratch, &system).unwrap();
            assert_eq!(compiler.executable, Path::new("compiler-helper"));
            let generated = system
                .filesystem
                .read_to_string(scratch.join("compiler").join("src").join("main.rs"))
                .unwrap();
            assert!(generated.contains(&format!("const WRAPPER: Option<&str> = {wrapper:?};")));
            let state = system.process.fake_state();
            let state = state.lock().unwrap();
            assert_eq!(state.requests.len(), 2);
            for request in &state.requests {
                for variable in ["RUSTC_WORKSPACE_WRAPPER", "RUSTC_WRAPPER"] {
                    assert!(!request.env_remove.contains(&OsStr::new(variable).to_os_string()));
                    assert!(!request.env.iter().any(|(key, _)| key == variable));
                }
            }
        }
    }
}
