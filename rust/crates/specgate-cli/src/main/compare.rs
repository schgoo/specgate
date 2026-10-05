use super::{EXIT_FAILURE, argument_error, format_comparison, output_error};
use specgate_ctsc::compare;
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(any(test, feature = "test-util"))]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the feature-gated process adapter is used by external test builds")
)]
pub(super) fn cmd_compare<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>) -> ExitCode {
    let stdout = std::io::stdout();
    compare_to(args, &mut stdout.lock(), &mut std::io::stderr().lock())
}

pub(super) fn compare_to<T: AsRef<std::ffi::OsStr>>(
    args: impl AsRef<[T]>,
    output: &mut impl std::io::Write,
    errors: &mut impl std::io::Write,
) -> ExitCode {
    let args = args.as_ref();
    let mut positional = Vec::with_capacity(args.len());
    let mut registry = None;
    let mut imports = Vec::with_capacity(args.len() / 2);
    let mut index = 0;
    while index < args.len() {
        match args[index].as_ref().to_str() {
            Some(flag @ ("--registry" | "--import")) => {
                let Some(value) = args.get(index + 1) else {
                    return argument_error(format!("{flag} needs an argument"), errors);
                };
                if flag == "--registry" {
                    if registry.replace(PathBuf::from(value)).is_some() {
                        return argument_error("--registry may be supplied only once", errors);
                    }
                } else {
                    imports.push(PathBuf::from(value));
                }
                index += 2;
            }
            Some(value) if !value.starts_with('-') => {
                positional.push(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
            Some(value) => return argument_error(format!("unexpected argument '{value}'"), errors),
            None => {
                positional.push(PathBuf::from(args[index].as_ref()));
                index += 1;
            }
        }
    }
    if positional.len() != 2 {
        return argument_error(
            "compare requires <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...",
            errors,
        );
    }
    if registry.is_none() && !imports.is_empty() {
        return argument_error("--import requires --registry", errors);
    }
    let report = compare(&positional[0], &positional[1], registry.as_deref(), &imports);
    if let Err(error) = output.write_all(format_comparison(&report).as_bytes()) {
        return output_error(format!("failed to write comparison report: {error}"), errors);
    }
    if report.equivalent {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILURE)
    }
}
