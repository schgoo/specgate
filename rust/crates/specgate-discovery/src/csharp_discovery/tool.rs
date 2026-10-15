//! Locating the prebuilt `SpecGate.Discovery` reflection tool.

use super::{failure, with_source};
use crate::error::Error;
use crate::runner::system::System;
use std::path::{Path, PathBuf};

/// Overrides where discovery looks for the reflection tool.
///
/// In a source checkout the `specgate` binary lives in the Rust target
/// directory, which is not where the C# tool is published, so the gate sets this
/// explicitly.
const DISCOVERY_PATH_VAR: &str = "SPECGATE_DISCOVERY_PATH";

/// Managed assembly of the prebuilt reflection tool, launched through `dotnet`.
///
/// The framework-dependent assembly is used rather than the native apphost so
/// one resolution rule works on every platform.
const DISCOVERY_ASSEMBLY: &str = "SpecGate.Discovery.dll";

/// Resolve the `SpecGate.Discovery` assembly.
///
/// `SPECGATE_DISCOVERY_PATH` wins when set, naming either the assembly itself or
/// the directory holding it. Otherwise the tool is expected beside the running
/// `specgate` executable, which is how a published install is laid out.
///
/// # Errors
///
/// Returns an error when the override names a missing assembly, when the running
/// executable's own path cannot be read, or when no assembly sits beside it.
pub(super) fn discovery_assembly(system: &System) -> Result<PathBuf, Error> {
    if let Some(value) = system.environment.var_os(DISCOVERY_PATH_VAR) {
        let candidate = resolve_override(PathBuf::from(value));
        if !system.filesystem.exists(&candidate) {
            return Err(failure(format!(
                "{DISCOVERY_PATH_VAR} names no {DISCOVERY_ASSEMBLY} at {}",
                candidate.display()
            )));
        }
        return Ok(candidate);
    }

    let executable = system
        .executable_path
        .get()
        .map_err(|source| with_source(format!("failed to locate the running specgate executable: {source}"), source))?;
    let directory = executable.parent().unwrap_or_else(|| Path::new("."));
    let candidate = directory.join(DISCOVERY_ASSEMBLY);
    if !system.filesystem.exists(&candidate) {
        return Err(failure(format!(
            "C# discovery requires {DISCOVERY_ASSEMBLY} beside the specgate executable at {}; set {DISCOVERY_PATH_VAR} to its location",
            directory.display()
        )));
    }
    Ok(candidate)
}

/// Accept either the assembly path or the directory containing it.
fn resolve_override(value: PathBuf) -> PathBuf {
    if value.extension().and_then(|extension| extension.to_str()) == Some("dll") {
        value
    } else {
        value.join(DISCOVERY_ASSEMBLY)
    }
}
