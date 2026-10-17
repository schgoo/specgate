use super::{ArgumentError, DEFAULT_BINDING, EXIT_FAILURE, argument_error, format_replay, implementation_label};
use specgate::ImplementationLabel;
use specgate_cli::replay::{Error as ReplayError, Paths, Report as ReplayReport, Request as ReplayRequest, replay};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ReplayArgs {
    pub(super) capture_dir: PathBuf,
    pub(super) binding: PathBuf,
    pub(super) implementation: Option<ImplementationLabel>,
    pub(super) out: Option<PathBuf>,
    pub(super) force: bool,
}

pub(super) fn parse_replay<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<ReplayArgs, ArgumentError> {
    let args = args.as_ref();
    let mut capture_dir = None;
    let mut binding = None;
    let mut implementation = None;
    let mut out = None;
    let mut force = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some("--force") => {
                force = true;
                index += 1;
            }
            Some(flag @ ("--binding" | "--impl" | "--out")) => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?.as_ref();
                match flag {
                    "--binding" => binding = Some(PathBuf::from(value)),
                    "--impl" => implementation = Some(implementation_label(flag, value)?),
                    _ => out = Some(PathBuf::from(value)),
                }
                index += 2;
            }
            Some(value) if value.starts_with('-') => return Err(ArgumentError::from(format!("unexpected argument '{value}'"))),
            _ if capture_dir.is_none() => {
                capture_dir = Some(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            Some(value) => return Err(ArgumentError::from(format!("unexpected argument '{value}'"))),
            None => return Err(ArgumentError::from("unexpected non-UTF-8 argument")),
        }
    }
    Ok(ReplayArgs {
        capture_dir: capture_dir.ok_or("replay requires <capture-dir>")?,
        binding: binding.unwrap_or_else(|| PathBuf::from(DEFAULT_BINDING)),
        implementation,
        out,
        force,
    })
}

pub(super) fn cmd_replay<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
) -> ExitCode {
    run_replay(args, output, errors, replay)
}

fn run_replay<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
    execute: impl FnOnce(ReplayRequest) -> Result<ReplayReport, ReplayError>,
) -> ExitCode {
    let args = args.as_ref();
    let parsed = match parse_replay(args) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error, errors),
    };
    let builder = ReplayRequest::builder(Paths::new(&parsed.capture_dir, &parsed.binding)).force(parsed.force);
    let builder = match parsed.out {
        Some(out) => builder.out(out),
        None => builder,
    };
    let builder = match parsed.implementation {
        Some(label) => builder.implementation(label),
        None => builder,
    };
    let outcome = builder.build().and_then(execute);
    if write!(output, "{}", format_replay(&outcome)).is_err() {
        return super::output_error("failed to write replay report", errors);
    }
    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILURE),
    }
}
