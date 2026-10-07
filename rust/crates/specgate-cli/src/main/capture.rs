use super::{ArgumentError, argument_error, format_capture, operation_status};
use specgate::{ComponentId, TargetName};
use specgate_cli::{CaptureError, CapturePaths, CaptureReport, CaptureRequest, capture};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CaptureArgs {
    pub(super) binding: PathBuf,
    pub(super) target: TargetName,
    pub(super) component: ComponentId,
    pub(super) out: PathBuf,
}

pub(super) fn parse_capture<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<CaptureArgs, ArgumentError> {
    let args = args.as_ref();
    let mut binding = None;
    let mut target = TargetName::default();
    let mut component = ComponentId::default();
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some(flag @ ("--target" | "--component" | "--out")) => {
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
                        component = ComponentId::from(
                            value
                                .as_ref()
                                .to_str()
                                .ok_or_else(|| "--component value is not valid UTF-8".to_string())?,
                        );
                    }
                    "--out" => out = Some(PathBuf::from(value.as_ref())),
                    _ => unreachable!("capture parser matched a flag outside its declared flag set"),
                }
                index += 2;
            }
            Some(value) if !value.starts_with('-') && binding.is_none() => {
                binding = Some(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            Some(value) => return Err(ArgumentError::from(format!("unexpected argument '{value}'"))),
            None if binding.is_none() => {
                binding = Some(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            None => return Err(ArgumentError::from("unexpected non-UTF-8 argument")),
        }
    }
    Ok(CaptureArgs {
        binding: binding.ok_or_else(|| "capture requires a binding file argument".to_string())?,
        target,
        component,
        out: out.ok_or_else(|| "capture requires --out <dir>".to_string())?,
    })
}

pub(super) fn cmd_capture<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
) -> ExitCode {
    run_with(args, output, errors, capture)
}

fn run_with<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
    execute: impl FnOnce(CaptureRequest) -> Result<CaptureReport, CaptureError>,
) -> ExitCode {
    let args = args.as_ref();
    let parsed = match parse_capture(args) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error, errors),
    };
    let outcome = CaptureRequest::builder(CapturePaths {
        binding: parsed.binding,
        out: parsed.out,
    })
    .target(parsed.target)
    .component(parsed.component)
    .build()
    .and_then(execute);
    if write!(output, "{}", format_capture(&outcome)).is_err() {
        return super::output_error("failed to write capture report", errors);
    }
    operation_status(outcome.is_ok())
}
