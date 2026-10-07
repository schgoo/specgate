use super::{ArgumentError, EXIT_FAILURE, argument_error, format_replay};
use specgate::TargetName;
use specgate_cli::replay::{Error as ReplayError, Paths, Report as ReplayReport, Request as ReplayRequest, replay};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ReplayArgs {
    pub(super) capture_dir: PathBuf,
    pub(super) binding: PathBuf,
    pub(super) target: TargetName,
    pub(super) out: PathBuf,
}

pub(super) fn parse_replay<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<ReplayArgs, ArgumentError> {
    let args = args.as_ref();
    let mut positional = Vec::with_capacity(args.len());
    let mut target = TargetName::default();
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some(flag @ ("--target" | "--out")) => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?;
                if flag == "--target" {
                    target = TargetName::from(
                        value
                            .as_ref()
                            .to_str()
                            .ok_or_else(|| "--target value is not valid UTF-8".to_string())?,
                    );
                } else {
                    out = Some(PathBuf::from(value.as_ref()));
                }
                index += 2;
            }
            Some(value) if !value.starts_with('-') => {
                positional.push(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            Some(value) => return Err(ArgumentError::from(format!("unexpected argument '{value}'"))),
            None => {
                positional.push(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
        }
    }
    if positional.len() != 2 {
        return Err(ArgumentError::from("replay requires <capture-dir> and <candidate-binding.yaml>"));
    }
    let capture_dir = positional.remove(0);
    let binding = positional.remove(0);
    let out = out.ok_or_else(|| "replay requires --out <candidate.otlp.json>".to_string())?;
    Ok(ReplayArgs {
        capture_dir,
        binding,
        target,
        out,
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
    let outcome = ReplayRequest::builder(Paths::new(&parsed.capture_dir, &parsed.binding, &parsed.out))
        .target(parsed.target)
        .build()
        .and_then(execute);
    if write!(output, "{}", format_replay(&outcome)).is_err() {
        return super::output_error("failed to write replay report", errors);
    }
    match outcome {
        Ok(_) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(EXIT_FAILURE),
    }
}
