use super::{ArgumentError, DEFAULT_BINDING, argument_error, flag_text, format_capture, implementation_label, operation_status};
use specgate::{ComponentId, ImplementationLabel};
use specgate_cli::{CaptureError, CapturePaths, CaptureReport, CaptureRequest, capture};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, PartialEq, Eq)]
pub(super) struct CaptureArgs {
    pub(super) binding: PathBuf,
    pub(super) implementation: Option<ImplementationLabel>,
    pub(super) component: ComponentId,
    pub(super) out: Option<PathBuf>,
    pub(super) force: bool,
}

pub(super) fn parse_capture<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<CaptureArgs, ArgumentError> {
    let args = args.as_ref();
    let mut binding = None;
    let mut implementation = None;
    let mut component = None;
    let mut out = None;
    let mut force = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some("--force") => {
                force = true;
                index += 1;
            }
            Some(flag @ ("--binding" | "--impl" | "--component" | "--out")) => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?.as_ref();
                match flag {
                    "--binding" => binding = Some(PathBuf::from(value)),
                    "--impl" => implementation = Some(implementation_label(flag, value)?),
                    "--component" => component = Some(ComponentId::from(flag_text(flag, value)?)),
                    "--out" => out = Some(PathBuf::from(value)),
                    _ => unreachable!("capture parser matched a flag outside its declared flag set"),
                }
                index += 2;
            }
            Some(value) => return Err(ArgumentError::from(format!("unexpected argument '{value}'"))),
            None => return Err(ArgumentError::from("unexpected non-UTF-8 argument")),
        }
    }
    Ok(CaptureArgs {
        binding: binding.unwrap_or_else(|| PathBuf::from(DEFAULT_BINDING)),
        implementation,
        component: component.ok_or_else(|| "capture requires --component <id>".to_string())?,
        out,
        force,
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
    let builder = CaptureRequest::builder(CapturePaths { binding: parsed.binding })
        .component(parsed.component)
        .force(parsed.force);
    let builder = match parsed.out {
        Some(out) => builder.out(out),
        None => builder,
    };
    let builder = match parsed.implementation {
        Some(label) => builder.implementation(label),
        None => builder,
    };
    let outcome = builder.build().and_then(execute);
    if write!(output, "{}", format_capture(&outcome)).is_err() {
        return super::output_error("failed to write capture report", errors);
    }
    if let Ok(report) = &outcome {
        for warning in report.warnings() {
            if writeln!(errors, "warning: {warning}").is_err() {
                return super::output_error("failed to write capture warning", errors);
            }
        }
    }
    operation_status(outcome.is_ok())
}
