//! `specgate` CLI binary entry point.

use std::path::PathBuf;
use std::process::ExitCode;

use specgate_cli::{capture, discover, replay};
use specgate_ctsc::comparison::compare;
use specgate_ctsc::validation::{validate_bundle, validate_linked, validate_registry, validate_trace};

fn print_usage() {
    eprintln!(
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json> [--target <name>]\n  capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]\n  replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]\n  validate registry <registry.json> [--import <registry.json>]...\n  validate trace <trace.otlp.json|trace.otlp.jsonl>\n  validate linked <trace> <root-registry> [--import <registry.json>]...\n  validate bundle <capture-dir>\n  compare <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]..."
    );
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_usage();
        return ExitCode::from(2);
    }
    match args[0].as_str() {
        "discover" => cmd_discover(&args[1..]),
        "capture" => cmd_capture(&args[1..]),
        "replay" => cmd_replay(&args[1..]),
        "validate" => cmd_validate(&args[1..]),
        "compare" => cmd_compare(&args[1..]),
        "-h" | "--help" => {
            print_usage();
            ExitCode::SUCCESS
        }
        command => {
            eprintln!("error: unknown command '{command}'");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn cmd_validate(args: &[String]) -> ExitCode {
    let Some(kind) = args.first() else {
        return argument_error("validate requires registry, trace, linked, or bundle");
    };
    let (positional, imports) = match parse_paths_and_imports(&args[1..]) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error),
    };
    let report = match kind.as_str() {
        "registry" if positional.len() == 1 => validate_registry(&positional[0], &imports),
        "trace" if positional.len() == 1 && imports.is_empty() => validate_trace(&positional[0]),
        "linked" if positional.len() == 2 => validate_linked(&positional[0], &positional[1], &imports),
        "bundle" if positional.len() == 1 && imports.is_empty() => validate_bundle(&positional[0]),
        "registry" => return argument_error("validate registry requires <registry.json> [--import <registry.json>]..."),
        "trace" => return argument_error("validate trace requires <trace.otlp.json|trace.otlp.jsonl>"),
        "linked" => {
            return argument_error("validate linked requires <trace> <root-registry> [--import <registry.json>]...");
        }
        "bundle" => return argument_error("validate bundle requires <capture-dir>"),
        _ => return argument_error("validate requires registry, trace, linked, or bundle"),
    };
    print!("{}", specgate_cli::validation::format_report(&report));
    if report.valid { ExitCode::SUCCESS } else { ExitCode::from(1) }
}

fn parse_paths_and_imports(args: &[String]) -> Result<(Vec<PathBuf>, Vec<PathBuf>), String> {
    let mut positional = Vec::new();
    let mut imports = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if args[index] == "--import" {
            let value = args.get(index + 1).ok_or_else(|| "--import needs an argument".to_string())?;
            imports.push(PathBuf::from(value));
            index += 2;
        } else if args[index].starts_with('-') {
            return Err(format!("unexpected argument '{}'", args[index]));
        } else {
            positional.push(PathBuf::from(&args[index]));
            index += 1;
        }
    }
    Ok((positional, imports))
}

fn cmd_compare(args: &[String]) -> ExitCode {
    let mut positional = Vec::new();
    let mut registry = None;
    let mut imports = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            flag @ ("--registry" | "--import") => {
                let Some(value) = args.get(index + 1) else {
                    return argument_error(&format!("{flag} needs an argument"));
                };
                if flag == "--registry" {
                    if registry.replace(PathBuf::from(value)).is_some() {
                        return argument_error("--registry may be supplied only once");
                    }
                } else {
                    imports.push(PathBuf::from(value));
                }
                index += 2;
            }
            value if !value.starts_with('-') => {
                positional.push(PathBuf::from(value));
                index += 1;
            }
            value => return argument_error(&format!("unexpected argument '{value}'")),
        }
    }
    if positional.len() != 2 {
        return argument_error(
            "compare requires <reference-trace> <candidate-trace> [--registry <root-registry>] [--import <registry.json>]...",
        );
    }
    if registry.is_none() && !imports.is_empty() {
        return argument_error("--import requires --registry");
    }
    let report = compare(&positional[0], &positional[1], registry.as_deref(), &imports);
    print!("{}", specgate_cli::comparison::format_report(&report));
    if report.equivalent { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
#[derive(Debug, PartialEq, Eq)]
struct ReplayArgs {
    capture_dir: String,
    binding: String,
    target: String,
    out: String,
}

fn parse_replay_args(args: &[String]) -> Result<ReplayArgs, String> {
    let mut positional = Vec::new();
    let mut target = String::new();
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            flag @ ("--target" | "--out") => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?.clone();
                if flag == "--target" {
                    target = value;
                } else {
                    out = Some(value);
                }
                index += 2;
            }
            value if !value.starts_with('-') => {
                positional.push(value.to_string());
                index += 1;
            }
            value => return Err(format!("unexpected argument '{value}'")),
        }
    }
    if positional.len() != 2 {
        return Err("replay requires <capture-dir> and <candidate-binding.yaml>".to_string());
    }
    Ok(ReplayArgs {
        capture_dir: positional.remove(0),
        binding: positional.remove(0),
        target,
        out: out.ok_or_else(|| "replay requires --out <candidate.otlp.json>".to_string())?,
    })
}

fn cmd_replay(args: &[String]) -> ExitCode {
    let parsed = match parse_replay_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error),
    };
    let outcome = replay(&parsed.capture_dir, &parsed.binding, &parsed.target, &parsed.out);
    print!("{}", replay::format_outcome(&outcome));
    match outcome {
        replay::ReplayOutcome::Complete { .. } => ExitCode::SUCCESS,
        replay::ReplayOutcome::Error { .. } => ExitCode::from(1),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct CaptureArgs {
    binding: String,
    target: String,
    component: String,
    out: String,
}

fn parse_capture_args(args: &[String]) -> Result<CaptureArgs, String> {
    let mut binding = None;
    let mut target = String::new();
    let mut component = String::new();
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            flag @ ("--target" | "--component" | "--out") => {
                let value = args.get(index + 1).ok_or_else(|| format!("{flag} needs an argument"))?.clone();
                match flag {
                    "--target" => target = value,
                    "--component" => component = value,
                    "--out" => out = Some(value),
                    _ => unreachable!(),
                }
                index += 2;
            }
            value if !value.starts_with('-') && binding.is_none() => {
                binding = Some(value.to_string());
                index += 1;
            }
            value => return Err(format!("unexpected argument '{value}'")),
        }
    }
    Ok(CaptureArgs {
        binding: binding.ok_or_else(|| "capture requires a binding file argument".to_string())?,
        target,
        component,
        out: out.ok_or_else(|| "capture requires --out <dir>".to_string())?,
    })
}

fn cmd_capture(args: &[String]) -> ExitCode {
    let parsed = match parse_capture_args(args) {
        Ok(parsed) => parsed,
        Err(error) => return argument_error(&error),
    };
    let outcome = capture(&parsed.binding, &parsed.target, &parsed.component, &parsed.out);
    print!("{}", capture::format_outcome(&outcome));
    match outcome {
        capture::CaptureOutcome::Complete { .. } => ExitCode::SUCCESS,
        capture::CaptureOutcome::Error { .. } => ExitCode::from(1),
    }
}

fn cmd_discover(args: &[String]) -> ExitCode {
    let mut binding = None;
    let mut target = String::new();
    let mut component = None;
    let mut registry_id = None;
    let mut registry_version = None;
    let mut out = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            flag @ ("--target" | "--component" | "--registry-id" | "--registry-version" | "-o" | "--out") => {
                let Some(value) = args.get(index + 1).cloned() else {
                    return argument_error(&format!("{flag} needs an argument"));
                };
                match flag {
                    "--target" => target = value,
                    "--component" => component = Some(value),
                    "--registry-id" => registry_id = Some(value),
                    "--registry-version" => registry_version = Some(value),
                    "-o" | "--out" => out = Some(value),
                    _ => unreachable!(),
                }
                index += 2;
            }
            value if !value.starts_with('-') && binding.is_none() => {
                binding = Some(value.to_string());
                index += 1;
            }
            value => return argument_error(&format!("unexpected argument '{value}'")),
        }
    }
    let Some(binding) = binding else {
        return argument_error("discover requires a binding file argument");
    };
    let Some(component) = component else {
        return argument_error("discover requires --component <id>");
    };
    let Some(registry_id) = registry_id else {
        return argument_error("discover requires --registry-id <id>");
    };
    let Some(registry_version) = registry_version else {
        return argument_error("discover requires --registry-version <version>");
    };
    let Some(out) = out else {
        return argument_error("discover requires -o/--out <registry.ctsc.json>");
    };
    let outcome = discover(&binding, &target, &component, &registry_id, &registry_version, &out);
    print!("{}", discover::format_outcome(&outcome));
    match outcome {
        discover::DiscoverOutcome::Complete { .. } => ExitCode::SUCCESS,
        discover::DiscoverOutcome::Error { .. } => ExitCode::from(1),
    }
}

fn argument_error(error: &str) -> ExitCode {
    eprintln!("error: {error}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn parses_capture_and_replay_arguments() {
        assert_eq!(
            parse_capture_args(&args(&["binding.yaml", "--out", "capture", "--component", "fixture.add"])).unwrap(),
            CaptureArgs {
                binding: "binding.yaml".to_string(),
                target: String::new(),
                component: "fixture.add".to_string(),
                out: "capture".to_string(),
            }
        );
        assert_eq!(
            parse_replay_args(&args(&["capture", "candidate.yaml", "--out", "candidate.json"])).unwrap(),
            ReplayArgs {
                capture_dir: "capture".to_string(),
                binding: "candidate.yaml".to_string(),
                target: String::new(),
                out: "candidate.json".to_string(),
            }
        );
    }

    #[test]
    fn required_outputs_are_enforced() {
        assert_eq!(
            parse_capture_args(&args(&["binding.yaml"])).unwrap_err(),
            "capture requires --out <dir>"
        );
        assert_eq!(
            parse_replay_args(&args(&["capture", "candidate.yaml"])).unwrap_err(),
            "replay requires --out <candidate.otlp.json>"
        );
    }
}
