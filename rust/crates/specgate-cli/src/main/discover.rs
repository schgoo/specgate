//! Discover-command parsing, execution injection, and stable CLI reporting.

use super::{ArgumentError, EXIT_FAILURE, argument_error};
use specgate::{ComponentId, TargetName};
use specgate_cli::discover::{
    self, ComponentName, Error as DiscoverError, Params, RegistryId, RegistryVersion, Report as DiscoverReport, Request as DiscoverRequest,
    discover,
};
use std::path::PathBuf;
use std::process::ExitCode;

/// Fully validated arguments for one discover command.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct DiscoverArgs {
    pub(super) binding: PathBuf,
    pub(super) target: TargetName,
    pub(super) component: ComponentId,
    pub(super) registry_id: RegistryId,
    pub(super) registry_version: RegistryVersion,
    pub(super) out: PathBuf,
}

/// Parse discover flags and the required binding path.
///
/// Returns an argument error for missing, duplicate-position, non-UTF-8, or unknown input.
pub(super) fn parse_discover<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<DiscoverArgs, ArgumentError> {
    let args = args.as_ref();
    let mut binding = None;
    let mut target = TargetName::default();
    let mut component = None;
    let mut registry_id = None;
    let mut registry_version = None;
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some(flag @ ("--target" | "--component" | "--registry-id" | "--registry-version" | "-o" | "--out")) => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?;
                match flag {
                    "--target" => {
                        target = TargetName::from(
                            value
                                .as_ref()
                                .to_str()
                                .ok_or_else(|| "--target value is not valid UTF-8".to_string())?,
                        );
                    }
                    "--component" => {
                        component = Some(ComponentId::from(
                            value
                                .as_ref()
                                .to_str()
                                .ok_or_else(|| "--component value is not valid UTF-8".to_string())?,
                        ));
                    }
                    "--registry-id" => {
                        registry_id = Some(RegistryId::parse(
                            value
                                .as_ref()
                                .to_str()
                                .ok_or_else(|| "--registry-id value is not valid UTF-8".to_string())?,
                        )?);
                    }
                    "--registry-version" => {
                        registry_version = Some(RegistryVersion::parse(
                            value
                                .as_ref()
                                .to_str()
                                .ok_or_else(|| "--registry-version value is not valid UTF-8".to_string())?,
                        )?);
                    }
                    "-o" | "--out" => out = Some(PathBuf::from(value.as_ref())),
                    _ => unreachable!("discover parser matched a flag outside its declared flag set"),
                }
                index += 2;
            }
            Some(value) if !value.starts_with('-') && binding.is_none() => {
                binding = Some(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            Some(value) => return Err(format!("unexpected argument '{value}'").into()),
            None if binding.is_none() => {
                binding = Some(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            None => return Err("unexpected non-UTF-8 argument".into()),
        }
    }
    Ok(DiscoverArgs {
        binding: binding.ok_or("discover requires a binding file argument")?,
        target,
        component: component.ok_or("discover requires --component <id>")?,
        registry_id: registry_id.ok_or("discover requires --registry-id <id>")?,
        registry_version: registry_version.ok_or("discover requires --registry-version <version>")?,
        out: out.ok_or("discover requires -o/--out <registry.ctsc.json>")?,
    })
}

/// Execute discovery with the real discover facade and write its stable outcome.
pub(super) fn cmd_discover<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: impl std::io::Write,
    errors: &mut impl std::io::Write,
) -> ExitCode {
    run_discover(args, output, errors, discover)
}

/// Execute discovery through an injected facade function for deterministic tests.
fn run_discover<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    mut output: impl std::io::Write,
    errors: &mut impl std::io::Write,
    execute: impl FnOnce(DiscoverRequest) -> Result<DiscoverReport, DiscoverError>,
) -> ExitCode {
    let parsed = match parse_discover(args) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(error, errors),
    };
    let outcome = (|| {
        DiscoverRequest::builder(Params {
            binding: parsed.binding,
            out: parsed.out,
            component: ComponentName::parse(parsed.component.as_str())?,
            registry_id: parsed.registry_id,
            registry_version: parsed.registry_version,
        })
        .target(parsed.target)
        .build()
        .and_then(execute)
    })();
    if let Err(error) = write!(output, "{}", discover::format_outcome(&outcome)) {
        return super::output_error(error, errors);
    }
    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILURE),
    }
}
