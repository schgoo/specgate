//! Discover-command parsing, execution injection, and stable CLI reporting.

use super::{ArgumentError, DEFAULT_BINDING, EXIT_FAILURE, argument_error, flag_text, implementation_label};
use specgate::{ComponentId, ImplementationLabel};
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
    pub(super) implementation: Option<ImplementationLabel>,
    pub(super) component: ComponentId,
    pub(super) registry_id: RegistryId,
    pub(super) registry_version: RegistryVersion,
    pub(super) out: PathBuf,
}

/// Parse discover flags.
///
/// Returns an argument error for missing, positional, non-UTF-8, or unknown input.
pub(super) fn parse_discover<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<DiscoverArgs, ArgumentError> {
    let args = args.as_ref();
    let mut binding = None;
    let mut implementation = None;
    let mut component = None;
    let mut registry_id = None;
    let mut registry_version = None;
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some(flag @ ("--binding" | "--impl" | "--component" | "--registry-id" | "--registry-version" | "-o" | "--out")) => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?.as_ref();
                match flag {
                    "--binding" => binding = Some(PathBuf::from(value)),
                    "--impl" => implementation = Some(implementation_label(flag, value)?),
                    "--component" => component = Some(ComponentId::from(flag_text(flag, value)?)),
                    "--registry-id" => registry_id = Some(RegistryId::parse(flag_text(flag, value)?)?),
                    "--registry-version" => registry_version = Some(RegistryVersion::parse(flag_text(flag, value)?)?),
                    "-o" | "--out" => out = Some(PathBuf::from(value)),
                    _ => unreachable!("discover parser matched a flag outside its declared flag set"),
                }
                index += 2;
            }
            Some(value) => return Err(format!("unexpected argument '{value}'").into()),
            None => return Err("unexpected non-UTF-8 argument".into()),
        }
    }
    Ok(DiscoverArgs {
        binding: binding.unwrap_or_else(|| PathBuf::from(DEFAULT_BINDING)),
        implementation,
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
        let builder = DiscoverRequest::builder(Params {
            binding: parsed.binding,
            out: parsed.out,
            component: ComponentName::parse(parsed.component.as_str())?,
            registry_id: parsed.registry_id,
            registry_version: parsed.registry_version,
        });
        let builder = match parsed.implementation {
            Some(label) => builder.implementation(label),
            None => builder,
        };
        builder.build().and_then(execute)
    })();
    if let Err(error) = write!(output, "{}", discover::format_outcome(&outcome)) {
        return super::output_error(error, errors);
    }
    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILURE),
    }
}
