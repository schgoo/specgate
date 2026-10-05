use super::{ArgumentError, EXIT_FAILURE, argument_error, format_validation, output_error};
use specgate_ctsc::validation::{validate_bundle, validate_linked, validate_registry, validate_trace};
use std::path::PathBuf;
use std::process::ExitCode;

pub(super) fn validate_to<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
) -> ExitCode {
    let args = args.as_ref();
    let Some(kind) = args.first() else {
        return argument_error("validate requires registry, trace, linked, or bundle", errors);
    };
    let (positional, imports) = match parse_inputs(&args[1..]) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error, errors),
    };
    let Some(kind) = kind.as_ref().to_str() else {
        return argument_error("validate kind is not valid UTF-8", errors);
    };
    let report = match kind {
        "registry" if positional.len() == 1 => validate_registry(&positional[0], &imports),
        "trace" if positional.len() == 1 && imports.is_empty() => validate_trace(&positional[0]),
        "linked" if positional.len() == 2 => validate_linked(&positional[0], &positional[1], &imports),
        "bundle" if positional.len() == 1 && imports.is_empty() => validate_bundle(&positional[0]),
        "registry" => return argument_error("validate registry requires <registry.json> [--import <registry.json>]...", errors),
        "trace" => return argument_error("validate trace requires <trace.otlp.json|trace.otlp.jsonl>", errors),
        "linked" => {
            return argument_error(
                "validate linked requires <trace> <root-registry> [--import <registry.json>]...",
                errors,
            );
        }
        "bundle" => return argument_error("validate bundle requires <capture-dir>", errors),
        _ => return argument_error("validate requires registry, trace, linked, or bundle", errors),
    };
    if let Err(error) = std::io::Write::write_all(output, format_validation(&report).as_bytes()) {
        return output_error(format!("failed to write validation report: {error}"), errors);
    }
    if report.valid {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILURE)
    }
}

pub(super) fn parse_inputs<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> Result<(Vec<PathBuf>, Vec<PathBuf>), ArgumentError> {
    let args = args.as_ref();
    let mut positional = Vec::with_capacity(args.len());
    let mut imports = Vec::with_capacity(args.len());
    let mut index = 0;
    while index < args.len() {
        if args[index].as_ref() == "--import" {
            let value = args
                .get(index + 1)
                .ok_or_else(|| ArgumentError::message("--import needs an argument"))?;
            imports.push(PathBuf::from(value.as_ref()));
            index += 2;
        } else if args[index].as_ref().to_string_lossy().starts_with('-') {
            return Err(ArgumentError::message(format!(
                "unexpected argument '{}'",
                args[index].as_ref().to_string_lossy()
            )));
        } else {
            positional.push(PathBuf::from(args[index].as_ref()));
            index += 1;
        }
    }
    Ok((positional, imports))
}
