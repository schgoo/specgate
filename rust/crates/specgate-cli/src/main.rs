//! `specgate` CLI binary entry point.

use std::process::ExitCode;

use specgate_cli::{capture, discover, replay};

fn print_usage() {
    eprintln!(
        "usage: specgate <command> [options] <args>\n\
         \n\
         commands:\n  discover <binding.yaml> --component <id> --registry-id <id> --registry-version <version> -o|--out <registry.ctsc.json> [--target <name>]\n  capture <binding.yaml> --out <dir> [--target <name>] [--component <id>]\n  replay <capture-dir> <candidate-binding.yaml> --out <candidate.otlp.json> [--target <name>]"
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
