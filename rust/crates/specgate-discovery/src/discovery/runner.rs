//! Rust generated-runner execution. The runner returns the target runtime
//! registry document as JSON and reports scaffold, Cargo, and output failures
//! through [`crate::Error`].

use super::{BTreeMap, Path};
use crate::error::{Error, ErrorKind};
use std::ffi::OsString;

fn failure(message: impl Into<String>) -> Error {
    Error::message(ErrorKind::Cargo, message)
}
fn with_source(message: impl Into<String>, source: Error) -> Error {
    Error::cause(ErrorKind::Cargo, message, source)
}

// ---------------------------------------------------------------------------
// Discovery build
// ---------------------------------------------------------------------------

/// Run native discovery for a candidate package.
///
/// A temporary binary links the target, prints `discovery_json()`, and returns
/// the captured registry document.
///
/// # Errors
///
/// Returns [`Error`] if the scaffold, build, or run fails, or if the
/// crate name cannot be read from `Cargo.toml`.
///
/// # Examples
///
/// ```no_run
/// use specgate_discovery::runner::run_discovery;
///
/// let registry_json = run_discovery("fixtures/rust-library")?;
/// assert!(registry_json.contains("\"operations\""));
/// # Ok::<(), specgate_discovery::Error>(())
/// ```
pub fn run_discovery(package_root: impl AsRef<Path>) -> Result<String, Error> {
    let context = crate::runner::candidate_package(package_root)?;
    run_with(&context)
}

pub(super) fn run_with(context: &crate::runner::CandidatePackage) -> Result<String, Error> {
    run_in(context, &crate::runner::system::System::real())
}

pub(super) fn run_in(context: &crate::runner::CandidatePackage, system: &crate::runner::system::System) -> Result<String, Error> {
    let scratch = crate::runner::cache::InvocationCache::create_with(
        crate::runner::cache::CacheScope::new("discovery"),
        crate::runner::cache::CacheLabel::new(&context.package),
        system,
    )?;
    system
        .filesystem
        .create_dir_all(scratch.path().join("src"))
        .map_err(|source| with_source(format!("failed to scaffold discovery crate: {source}"), source))?;

    let cargo = discovery_cargo(context)?;
    system
        .filesystem
        .write(scratch.path().join("Cargo.toml"), cargo.manifest)
        .map_err(|source| with_source(format!("failed to write discovery manifest: {source}"), source))?;
    let registry_config = cargo
        .config
        .map(|config| {
            let path = scratch.path().join("registry-config.toml");
            system
                .filesystem
                .write(&path, config)
                .map_err(|source| with_source(format!("failed to write discovery registry config: {source}"), source))?;
            Ok::<_, Error>(path)
        })
        .transpose()?;

    // `extern crate` forces the target crate's rlib to be linked so its
    // `linkme` registration statics (which are `#[used]`) are pulled in.
    let main_rs = "extern crate candidate;\nfn main() {\n    print!(\"{}\", specgate_runtime::registry::discovery());\n}\n";
    system
        .filesystem
        .write(scratch.path().join("src").join("main.rs"), main_rs)
        .map_err(|source| with_source(format!("failed to write discovery main.rs: {source}"), source))?;

    let request = crate::runner::system::ProcessRequest::builder(system_cargo(system))
        .arg("run")
        .arg("--quiet");
    let request = if let Some(config) = registry_config {
        request.arg("--config").arg(config)
    } else {
        request
    };
    let request = request
        .arg("--manifest-path")
        .arg(scratch.path().join("Cargo.toml"))
        .current_dir(&context.path)
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("CARGO")
        .env_remove("CARGO_MANIFEST_DIR")
        .env("CARGO_TARGET_DIR", scratch.path().join("target").as_os_str())
        .build();
    let output = system
        .process
        .output(&request)
        .map_err(|source| with_source(format!("failed to run discovery build: {source}"), source))?;
    if !output.success {
        let error = format!("discovery build failed: {}", String::from_utf8_lossy(&output.stderr).trim());
        return Err(failure(error));
    }
    let json = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if json.is_empty() {
        return Err(failure("discovery build produced no output"));
    }
    Ok(json)
}

pub(super) fn discovery_cargo(context: &crate::runner::CandidatePackage) -> Result<crate::runner::RunnerCargo, Error> {
    let dependencies = BTreeMap::from([
        (
            "candidate".to_string(),
            crate::runner::Dependency::local(context.package.clone(), context.version.clone(), context.path.clone())?,
        ),
        (
            "specgate_runtime".to_string(),
            crate::runner::Dependency::from_source(&context.runtime)?,
        ),
    ]);
    crate::runner::runner_cargo("specgate-discovery-runner", dependencies)
}

/// Return the Cargo executable selected by the current environment.
#[must_use]
pub fn cargo_bin() -> OsString {
    system_cargo(&crate::runner::system::System::real())
}
fn system_cargo(system: &crate::runner::system::System) -> OsString {
    crate::runner::cargo_with(system)
}
