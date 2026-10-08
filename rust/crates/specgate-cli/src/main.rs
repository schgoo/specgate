//! `specgate` CLI binary entry point.

#[path = "main/capture.rs"]
mod capture_command;
#[path = "main/compare.rs"]
mod compare_command;
#[path = "main/discover.rs"]
mod discover_command;
#[path = "main/replay.rs"]
mod replay_command;
mod reporting;
#[path = "main/validate.rs"]
mod validate_command;

use capture_command::cmd_capture;
#[cfg(test)]
use capture_command::{CaptureArgs, parse_capture};
#[cfg(test)]
use compare_command::cmd_compare;
use compare_command::compare_to;
use discover_command::cmd_discover;
#[cfg(test)]
use discover_command::{DiscoverArgs, parse_discover};
use replay_command::cmd_replay;
#[cfg(test)]
use replay_command::{ReplayArgs, parse_replay};
#[cfg(test)]
use validate_command::parse_inputs;
use validate_command::validate_to;

use reporting::{format_capture, format_comparison, format_replay, format_validation};

use std::process::ExitCode;

// Conventional process failure for a valid command whose operation failed.
const EXIT_FAILURE: u8 = 1;
// Conventional CLI usage error, distinct for shell automation.
const EXIT_USAGE: u8 = 2;

#[ohno::error]
#[display("{diagnostic}")]
struct ArgumentError {
    diagnostic: String,
    backtrace: std::backtrace::Backtrace,
}

impl ArgumentError {
    fn message(diagnostic: impl AsRef<str>) -> Self {
        Self::new(diagnostic.as_ref().to_owned(), std::backtrace::Backtrace::capture())
    }
}

impl From<String> for ArgumentError {
    fn from(diagnostic: String) -> Self {
        Self::new(diagnostic, std::backtrace::Backtrace::capture())
    }
}

impl From<specgate_cli::discover::Error> for ArgumentError {
    fn from(source: specgate_cli::discover::Error) -> Self {
        Self::caused_by(source.to_string(), std::backtrace::Backtrace::capture(), source)
    }
}

impl From<&str> for ArgumentError {
    fn from(diagnostic: &str) -> Self {
        Self::new(diagnostic.to_string(), std::backtrace::Backtrace::capture())
    }
}

impl AsRef<str> for ArgumentError {
    fn as_ref(&self) -> &str {
        &self.diagnostic
    }
}

#[cfg(test)]
impl PartialEq<&str> for ArgumentError {
    fn eq(&self, other: &&str) -> bool {
        self.diagnostic == *other
    }
}

fn operation_status(succeeded: bool) -> ExitCode {
    if succeeded {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_FAILURE)
    }
}

fn usage_status() -> ExitCode {
    ExitCode::from(EXIT_USAGE)
}

fn print_usage(writer: &mut impl std::io::Write) -> std::io::Result<()> {
    writeln!(
        writer,
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json> [--target <name>]\n  capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]\n  replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]\n  validate registry <registry.json> [--import <registry.json>]...\n  validate trace <trace.otlp.json|trace.otlp.jsonl>\n  validate linked <trace> <root-registry> [--import <registry.json>]...\n  validate bundle <capture-dir>\n  compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]..."
    )
}

fn usage_result(success_status: ExitCode, stderr: &mut impl std::io::Write) -> ExitCode {
    if print_usage(stderr).is_ok() {
        success_status
    } else {
        ExitCode::FAILURE
    }
}

fn run<T: AsRef<std::ffi::OsStr>>(args: impl AsRef<[T]>, stdout: &mut impl std::io::Write, stderr: &mut impl std::io::Write) -> ExitCode {
    let args = args.as_ref();
    if args.is_empty() {
        return usage_result(ExitCode::from(EXIT_USAGE), stderr);
    }
    let Some(command) = args[0].as_ref().to_str() else {
        return argument_error("command name is not valid UTF-8", stderr);
    };
    match command {
        "discover" => cmd_discover(&args[1..], stdout, stderr),
        "capture" => cmd_capture(&args[1..], stdout, stderr),
        "replay" => cmd_replay(&args[1..], stdout, stderr),
        "validate" => validate_to(&args[1..], stdout, stderr),
        "compare" => compare_to(&args[1..], stdout, stderr),
        "-h" | "--help" => usage_result(ExitCode::SUCCESS, stderr),
        command => {
            if writeln!(stderr, "error: unknown command '{command}'").is_err() {
                return ExitCode::FAILURE;
            }
            usage_result(ExitCode::from(EXIT_USAGE), stderr)
        }
    }
}

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    run(args, &mut std::io::stdout().lock(), &mut std::io::stderr().lock())
}

fn output_error(error: impl std::fmt::Display, writer: &mut impl std::io::Write) -> ExitCode {
    let _ = writeln!(writer, "error: {error}");
    ExitCode::from(EXIT_FAILURE)
}

fn argument_error(error: impl AsRef<str>, writer: &mut impl std::io::Write) -> ExitCode {
    let _ = writeln!(writer, "error: {}", error.as_ref());
    usage_status()
}

#[cfg(test)]
#[path = "main/tests.rs"]
mod tests;
