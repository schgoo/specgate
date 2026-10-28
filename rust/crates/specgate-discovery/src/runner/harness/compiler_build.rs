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
        "const ROOTS: &[&str] = &{roots:?};\nconst FACADES: &[&str] = &{:?};\ninclude!({:?});\n",
        config.facades, "compiler.rs"
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
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("SPECGATE_TEST_ENTRY")
        .env_remove("SPECGATE_TEST_DISPATCH")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", directory.join("target"))
        .build();
    let output = system
        .output(&request)
        .map_err(|source| Error::cause(crate::error::ErrorKind::Cargo, "failed to run test-harness compiler build", source))?;
    if !output.status.success() {
        return Err(failure(format!(
            "test-harness compiler build failed:\n{}",
            request.diagnostic(Some(&output), &system.environment)
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
